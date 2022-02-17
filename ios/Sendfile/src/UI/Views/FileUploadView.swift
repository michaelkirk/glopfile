import Foundation
import SwiftUI

struct FileUploadView: View {
    @StateObject var fileUpload: FileUploadModel

    var body: some View {
        ZStack(alignment: .center) {
            VStack {
                if case .error(_) = fileUpload.lastResult {
                    retryButton
                }
                uploadStatus
            }
            VStack {
                Spacer()
                provisionedUrlButton
            }
        }
    }

    var provisionedUrlButton: some View {
        VStack {
            if case .success = fileUpload.lastResult {
            } else if let provisionedUrl = fileUpload.provisionedUrl {
                Text("Tap to copy to clipboard:")
                Button(action: {
                    UIPasteboard.general.url = provisionedUrl
                }) {
                    Text("\(provisionedUrl)")
                }
            }
        }
        .padding()
    }

    var retryButton: some View {
        Button(action: {
            fileUpload.start()
        }) {
            Text("Retry")
        }
    }

    var uploadStatus: some View {
        VStack {
            if let lastResult = fileUpload.lastResult {
                switch lastResult {
                case .success:
                    Text("File sent!")
                case .error(let lastError):
                    Text("Error: \(lastError)")
                }
            } else if fileUpload.provisionedUrl == nil {
                Text("Provisioning...")
            } else {
                Text("Uploading...")
            }
        }
    }
}
