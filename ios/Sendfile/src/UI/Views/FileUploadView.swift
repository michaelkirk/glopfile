import ActivityView
import Foundation
import SwiftUI

struct FileUploadView: View {
    @Binding var fileUrl: URL?
    @State var shareDownloadUrl: ActivityItem?
    @StateObject var fileUpload: FileUploadModel = FileUploadModel()

    var body: some View {
        VStack(alignment: .center, spacing: 16) {
            uploadStatus
            switch fileUpload.state {
            case .error(_):
                retryButton
            case .provisioned(let fileUrl, let downloadUrl):
                shareView(fileUrl: fileUrl, downloadUrl: downloadUrl)
            default:
                EmptyView()
            }
            Spacer()
        }.padding()
            .onAppear {
                if let fileUrl = fileUrl, case .idle = fileUpload.state {
                    fileUpload.start(fileUrl: fileUrl)
                }
            }
    }

    func shareView(fileUrl: URL, downloadUrl: URL) -> some View {
        VStack {
            Text("Send the download link to the recipient to continue uploading:")
            Text(downloadUrl.absoluteString).lineLimit(1).padding(.bottom, 16)

            HStack(alignment: .center, spacing: 32) {
                Button(action: {
                    UIPasteboard.general.url = downloadUrl
                }) {
                    Label("Copy Link", systemImage: "doc.on.doc")
                }
                Button(action: {
                    self.shareDownloadUrl = ActivityItem(items: downloadUrl)
                }) {
                    Label("Share Link", systemImage: "square.and.arrow.up")
                }.activitySheet(self.$shareDownloadUrl)
            }
            Spacer()
            if let image = buildQRImage(url: downloadUrl) {
                Text("Or have them scan this code:")
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(
                        minWidth: nil, idealWidth: nil, maxWidth: 200, minHeight: nil,
                        idealHeight: nil, maxHeight: 200, alignment: .center)
                Spacer()
            }
        }
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
        func Header(_ text: String) -> some View {
            return Text(text).font(.headline)
        }
        return VStack {
            switch fileUpload.state {
            case .idle:
                EmptyView()
            case .started:
                Header("Provisioning...")
            case .provisioned:
                Header("Uploading...")
            case .success:
                Header("File sent!")
            case .error(let lastError):
                Header("Error: \(lastError.localizedDescription)")
            }
        }
    }
}

func buildQRImage(url: URL) -> UIImage? {
    guard let filter = CIFilter(name: "CIQRCodeGenerator") else {
        assertionFailure("failed to build QR Code generator")
        return nil
    }

    let data = url.absoluteString.data(using: .utf8)
    filter.setValue(data, forKey: "inputMessage")

    let scaleUp = CGAffineTransform(scaleX: 4, y: 4)
    guard let ciImage = filter.outputImage?.transformed(by: scaleUp) else {
        assertionFailure("failed to generate QR Code image")
        return nil
    }

    guard let renderedImage = CIContext().createCGImage(ciImage, from: ciImage.extent) else {
        assertionFailure("failed to render QR Code image")
        return nil
    }

    let image = UIImage(cgImage: renderedImage)
    return image
}
