import Foundation
import SwiftUI

struct FileUploadView: View {
    @Binding var fileUrl: URL?

    @StateObject var fileUpload: FileUploadModel = FileUploadModel()

    var body: some View {
        VStack(alignment: .center, spacing: 16) {
            uploadStatus
            if case .error(_) = fileUpload.state {
                retryButton
            }
            shareView
            Spacer()
        }
        .onAppear {
            if let fileUrl = fileUrl, case .idle = fileUpload.state {
                fileUpload.start(fileUrl: fileUrl)
            }
        }
    }

    var shareView: some View {
        VStack {
            if case .success = fileUpload.state {
            } else if let downloadUrl = fileUpload.pending?.provisionedFile?
                .formattedDownloadUrlAndKey()
            {
                Text("Send this download link to the recipient to continue uploading:")
                Button(action: {
                    UIPasteboard.general.url = URL(string: downloadUrl)!
                }) {
                    Text("\(downloadUrl)")
                }.padding(.bottom, 30)

                if let image = buildQRImage(urlString: downloadUrl) {
                    Text("Or have them scan this code:")
                    Image(uiImage: image)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(
                            minWidth: nil, idealWidth: nil, maxWidth: 200, minHeight: nil,
                            idealHeight: nil, maxHeight: 200, alignment: .center)
                }
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
                if fileUpload.pending == nil {
                    Header("Provisioning...")
                } else {
                    Header("Uploading...")
                }
            case .success:
                Header("File sent!")
            case .error(let lastError):
                Header("Error: \(lastError.localizedDescription)")
            }
        }
    }
}

func buildQRImage(urlString: String) -> UIImage? {
    guard let filter = CIFilter(name: "CIQRCodeGenerator") else {
        assertionFailure("failed to build QR Code generator")
        return nil
    }

    let data = urlString.data(using: .utf8)
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
