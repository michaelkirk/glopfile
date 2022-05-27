import Foundation
import SwiftUI

struct FileDownloadView: View {
    @Binding var url: URL?

    @StateObject var fileDownload: FileDownloadModel = FileDownloadModel()
    @State var fileViewerActive = false

    var body: some View {
        ZStack(alignment: .center) {
            VStack {
                switch fileDownload.state {
                case .error, .idle:
                    startButton
                default:
                    EmptyView()
                }

                HStack {
                    downloadStatus
                    if case .success = fileDownload.state {
                        shareButton
                    }
                }
            }
            VStack {
                Spacer()
            }
        }
        .background(
            Group {
                if case .success(let fileUrl) = fileDownload.state {
                    DocumentViewer(fileUrl: fileUrl, isPresented: $fileViewerActive)
                        .onAppear {
                            fileViewerActive = true
                        }
                }
            }
        )
        .onChange(of: url) { _ in
            if let url = url, url != fileDownload.pending?.url {
                fileDownload.startFetchMeta(url: url)
            }
        }
        .onAppear {
            if let url = url, case .idle = fileDownload.state {
                fileDownload.startFetchMeta(url: url)
            }
        }
    }

    var startButton: some View {
        Button(action: {
            if let url = url {
                fileDownload.startTransfer(url: url)
            }
        }) {
            switch fileDownload.state {
            case .idle:
                if let fileMeta = fileDownload.pending?.meta?.fileMetaFfi() {
                    let formattedFileSize = formatFileSize(bytes: fileMeta.fileSize)
                    Text("Download \(fileMeta.fileName) (\(formattedFileSize))")
                } else {
                    Text("Retry Fetching Download Information")
                }
            default:
                Text("Retry Download")
            }
        }
    }

    var downloadStatus: some View {
        VStack {
            switch fileDownload.state {
            case .idle:
                EmptyView()
            case .fetchingMeta:
                Text("Retrieving download information...")
            case .transferring:
                if let fileMeta = fileDownload.pending?.meta?.fileMetaFfi() {
                    let formattedFileSize = formatFileSize(bytes: fileMeta.fileSize)
                    Text("Downloading \(fileMeta.fileName) (\(formattedFileSize))...")
                } else {
                    Text("Downloading...")
                }
            case .success:
                Text("File downloaded!")
            case .error(let lastError):
                Text("Error: \(lastError.localizedDescription)")
            }
        }
    }

    var shareButton: some View {
        Button(action: {
            fileViewerActive = true
        }) {
            Image(systemName: "square.and.arrow.up")
                .foregroundColor(.accentColor)
        }
    }
}

private func formatFileSize(bytes: UInt64) -> String {
    let size = Measurement(value: Double(bytes), unit: UnitInformationStorage.bytes)
    let convertedSize = size.converted(to: .megabytes)
    return String.localizedStringWithFormat(
        "%0.2f %@", convertedSize.value, convertedSize.unit.symbol)
}
