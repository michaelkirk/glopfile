import Foundation
import SwiftUI

struct FileUploadView: View {
    @Binding var fileUrl: URL?

    @StateObject var fileUpload: FileUploadModel = FileUploadModel()

    var body: some View {
        ZStack(alignment: .center) {
            VStack {
                if case .error(_) = fileUpload.state {
                    retryButton
                }
                uploadStatus
            }
            VStack {
                Spacer()
                provisionedUrlButton
            }
        }
        .onAppear {
            if let fileUrl = fileUrl, case .idle = fileUpload.state {
                fileUpload.start(fileUrl: fileUrl)
            }
        }
    }

    var provisionedUrlButton: some View {
        VStack {
            if case .success = fileUpload.state {
            } else if let provisionedUrl = fileUpload.pending?.provisionedFile?.formattedDownloadUrlAndKey() {
                Text("Tap to copy to clipboard:")
                Button(action: {
                    UIPasteboard.general.url = URL(string: provisionedUrl)!
                }) {
                    Text("\(provisionedUrl)")
                }
            }
        }
        .padding()
    }

    var retryButton: some View {
        ZStack {
            if let fileUrl = fileUrl {
                Button(action: {
                    fileUpload.start(fileUrl: fileUrl)
                }) {
                    Text("Retry")
                }
            }
        }
    }

    var uploadStatus: some View {
        VStack {
            switch fileUpload.state {
            case .idle:
                EmptyView()
            case .started:
                if fileUpload.pending == nil {
                    Text("Provisioning...")
                } else {
                    Text("Uploading...")
                }
            case .success:
                Text("File sent!")
            case .error(let lastError):
                Text("Error: \(lastError.localizedDescription)")
            }
        }
    }
}
