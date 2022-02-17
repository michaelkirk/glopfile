import Foundation
import Combine
import os

enum FileUploadModelError: Error {
    case access
}

enum FileUploadResult {
    case success
    case error(String)
}

class FileUploadModel: ObservableObject {
    let fileUrl: URL

    @Published var lastResult: FileUploadResult?

    private let fileUploader: FileUploader
    @Published private var fileUpload: FileUpload?
    private let dispatchQueue = DispatchQueue(label: "FileUploadModel", attributes: .concurrent, target: .global(qos: .userInitiated))

    init(fileUrl: URL) throws {
        self.fileUrl = fileUrl
        if !fileUrl.startAccessingSecurityScopedResource() {
            throw FileUploadModelError.access
        }
        fileUploader = try FileUploader(apiEndpoint: "https://sendfile.jessa0.com/", downloadEndpoint: "https://s.endoftheworl.de/")
    }

    var provisionedUrl: URL? {
        fileUpload
            .flatMap { upload in upload.url() }
            .flatMap { url in URL(string: url) }
    }

    func start() {
        dispatchPrecondition(condition: .onQueue(DispatchQueue.main))
        let existingFileUpload = self.fileUpload
        dispatchQueue.async {
            let fileUpload: FileUpload
            if let existingFileUpload = existingFileUpload {
                fileUpload = existingFileUpload
            } else {
                do {
                    fileUpload = try self.fileUploader.provisionFile(path: self.fileUrl.path)
                    DispatchQueue.main.async {
                        self.fileUpload = fileUpload
                    }
                } catch let error {
                    os_log("error provisioning file: \(error.localizedDescription)")
                    DispatchQueue.main.async {
                        self.lastResult = .error(error.localizedDescription)
                    }
                    return
                }
            }

            let result: FileUploadResult
            do {
                try fileUpload.upload()
                result = .success
            } catch let error {
                os_log("error uploading file: \(error.localizedDescription)")
                result = .error(error.localizedDescription)
            }
            DispatchQueue.main.async {
                self.lastResult = result
            }
        }
    }

    deinit {
        fileUrl.stopAccessingSecurityScopedResource()
    }
}
