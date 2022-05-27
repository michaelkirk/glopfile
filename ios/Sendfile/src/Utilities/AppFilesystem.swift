import Dispatch
import Foundation

public class AppFilesystem {
    static func appTemporaryDirectory() throws -> URL {
        FileManager.default.temporaryDirectory
    }

    static func appDownloadDirectory(downloadId: String) throws -> URL {
        let downloadDirectory = try appTemporaryDirectory().appendingPathComponent(downloadId)
        if !FileManager.default.fileExists(atPath: downloadDirectory.path) {
            try FileManager.default.createDirectory(
                at: downloadDirectory, withIntermediateDirectories: false)
        }
        return downloadDirectory
    }
}
