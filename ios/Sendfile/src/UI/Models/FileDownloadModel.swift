import Foundation
import os
import SendfileRustFFI

class FileDownloadModel: ObservableObject {
    static let P2P_TIMEOUT: TimeInterval? = 5.0

    @Published var state: State = .idle
    @Published var pending: PendingFileDownload?

    enum State {
        case idle
        case fetchingMeta(url: URL)
        case transferring(url: URL)
        case success(localFileUrl: URL)
        case error(Error)
    }

    private let dispatchQueue = DispatchQueue(label: "FileDownloader", attributes: .concurrent, target: .global(qos: .userInitiated))

    func startFetchMeta(url: URL) {
        dispatchPrecondition(condition: .onQueue(DispatchQueue.main))

        state = .fetchingMeta(url: url)

        dispatchQueue.async {
            var pending: PendingFileDownload
            do {
                do {
                    pending = try PendingFileDownload(url: url)
                } catch let error {
                    DispatchQueue.main.async {
                        self.state = .error(error)
                        self.pending = nil
                    }
                    return
                }
                let _ = try pending.fetchMeta()
                DispatchQueue.main.async {
                    self.state = .idle
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

    func startTransfer(url: URL) {
        dispatchPrecondition(condition: .onQueue(DispatchQueue.main))

        var existingPending: PendingFileDownload?
        if let pending = pending, pending.url == url {
            existingPending = pending
            state = .transferring(url: url)
        } else {
            state = .fetchingMeta(url: url)
        }

        dispatchQueue.async {
            var pending: PendingFileDownload
            do {
                do {
                    if let existingPending = existingPending {
                        pending = existingPending
                    } else {
                        pending = try PendingFileDownload(url: url)

                        DispatchQueue.main.async {
                            self.state = .fetchingMeta(url: url)
                            self.pending = pending
                        }
                    }
                } catch let error {
                    DispatchQueue.main.async {
                        self.state = .error(error)
                        self.pending = nil
                    }
                    return
                }

                let _ = try pending.fetchMeta()
                DispatchQueue.main.async {
                    self.state = .transferring(url: url)
                    self.pending = pending
                }

                let localFileUrl = try pending.transfer()
                os_log("Downloaded \(localFileUrl)")

                DispatchQueue.main.async {
                    self.state = .success(localFileUrl: localFileUrl)
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

struct PendingFileDownload {
    let url: URL
    var meta: DownloadMeta?

    private let client: DownloaderClient

    fileprivate init(url: URL) throws {
        self.url = url
        do {
            client = try DownloaderClient.fromDownloadUrlFfi(
                downloadUrl: url.absoluteString,
                apiEndpoint: Constants.apiEndpoint,
                transport: .both
            )
        } catch let error {
            throw FileDownloadModelError.initialize(error)
        }
    }

    fileprivate mutating func fetchMeta() throws -> DownloadMeta {
        if let meta = meta {
            return meta
        }
        do {
            let meta = try client.fetchMetaFfi()
            self.meta = meta
            return meta
        } catch let error {
            throw FileDownloadModelError.fetchMeta(error)
        }
    }

    fileprivate mutating func transfer() throws -> URL {
        let meta = try fetchMeta()
        let downloadId = client.downloadIdFfi()
        let downloadDirectoryUrl: URL
        do {
            downloadDirectoryUrl = try AppFilesystem.appDownloadDirectory(downloadId: downloadId)
        } catch let error {
            throw FileDownloadModelError.io(error)
        }

        do {
            try client.downloadFfi(meta: meta, outputDir: downloadDirectoryUrl.path, p2pTimeout: FileDownloadModel.P2P_TIMEOUT)
        } catch let error {
            throw FileDownloadModelError.download(error)
        }
        
        return downloadDirectoryUrl.appendingPathComponent(meta.fileMetaFfi().fileName)
    }
}

enum FileDownloadModelError: Error {
    case initialize(Error)
    case fetchMeta(Error)
    case download(Error)
    case io(Error)

    var localizedDescription: String {
        switch (self) {
        case .initialize: return "Internal error starting download: \(message)"
        case .fetchMeta: return "Error retrieving download information: \(message)"
        case .download: return "Error downloading file: \(message)"
        case .io: return "Error writing file: \(message)"
        }
    }

    var message: String {
        switch (self) {
        case .initialize(let error), .fetchMeta(let error), .download(let error), .io(let error):
            if case let error as SendfileError = error {
                return error.message
            } else {
                return error.localizedDescription
            }
        }
    }
}
