import Foundation
import Combine
import os
import SendfileRustFFI

class FileUploadModel: ObservableObject {
    @Published var state: State = .idle
    @Published var pending: PendingFileUpload?

    enum State {
        case idle
        case started(URL)
        case success
        case error(Error)
    }

    private let dispatchQueue = DispatchQueue(label: "FileUploadModel", attributes: .concurrent, target: .global(qos: .userInitiated))

    func start(fileUrl: URL) {
        dispatchPrecondition(condition: .onQueue(DispatchQueue.main))

        var existingPending: PendingFileUpload?
        if let pending = pending, pending.fileUrl == fileUrl {
            existingPending = pending
        }

        state = .started(fileUrl)

        dispatchQueue.async {
            var pending: PendingFileUpload
            do {
                do {
                    if !fileUrl.startAccessingSecurityScopedResource() {
                        throw FileUploadModelError.access
                    }

                    defer {
                        fileUrl.stopAccessingSecurityScopedResource()
                    }

                    if let existingPending = existingPending {
                        pending = existingPending
                    } else {
                        pending = try PendingFileUpload(fileUrl: fileUrl)
                    }
                } catch let error {
                    DispatchQueue.main.async {
                        self.state = .error(error)
                        self.pending = nil
                    }
                    return
                }

                let _ = try pending.provisionFile()
                DispatchQueue.main.async {
                    self.state = .started(fileUrl)
                    self.pending = pending
                }

                try pending.transfer()

                DispatchQueue.main.async {
                    self.state = .success
                    self.pending = pending
                }
            } catch let error {
                DispatchQueue.main.async {
                    self.state = .error(error)
                    self.pending = pending
                }
            }
        }
    }
}

struct PendingFileUpload {
    let fileUrl: URL
    let client: UploaderClient
    var provisionedFile: NativeProvisionedFile?

    fileprivate init(fileUrl: URL) throws {
        self.fileUrl = fileUrl
        do {
            client = try UploaderClient.newFfi(
                apiEndpoint: Constants.apiEndpoint,
                downloadEndpoint: Constants.downloadEndpoint,
                transport: .both
            )
        } catch let error {
            throw FileUploadModelError.initialize(error)
        }
    }

    fileprivate mutating func provisionFile() throws -> NativeProvisionedFile {
        if let provisionedFile = provisionedFile {
            return provisionedFile
        }
        do {
            let provisionedFile = try client.provisionFileFfi(path: fileUrl.path)
            self.provisionedFile = provisionedFile
            return provisionedFile
        } catch let error {
            throw FileUploadModelError.provision(error)
        }
    }

    fileprivate mutating func transfer() throws {
        let provisionedFile = try provisionFile()
        do {
            try client.uploadProvisionedFileFfi(provisionedFile: provisionedFile)
        } catch let error {
            throw FileUploadModelError.upload(error)
        }
    }
}

enum FileUploadModelError: Error {
    case access
    case initialize(Error)
    case provision(Error)
    case upload(Error)

    var localizedDescription: String {
        switch (self) {
        case .access: return message
        case .initialize: return "Internal error starting upload: \(message)"
        case .provision: return "Error starting upload: \(message)"
        case .upload: return "Error uploading file: \(message)"
        }
    }

    var message: String {
        switch (self) {
        case .access:
            return "Cannot access file to upload."
        case .initialize(let error), .provision(let error), .upload(let error):
            if case let error as SendfileError = error {
                return error.message
            } else {
                return error.localizedDescription
            }
        }
    }
}
