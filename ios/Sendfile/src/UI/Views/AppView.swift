import Foundation
import SwiftUI
import os
import SendfileRustFFI

struct AppView: View {
    @Environment(\.env) var env: AppEnvironment

    @State var uploadFileUrl: URL?
    @State var uploadFilePickerPresented = false
    @State var fileUpload: FileUploadModel?
    @State var fileUploadViewIsActive = false

    var body: some View {
        NavigationView {
            ZStack {
                uploadView
                if let fileUpload = fileUpload {
                    NavigationLink(destination: FileUploadView(fileUpload: fileUpload), isActive: $fileUploadViewIsActive) {
                        EmptyView()
                    }
                }
            }
        }
    }

    var uploadView: some View {
        VStack(alignment: .center) {
            uploadFileButton
        }
        .onChange(of: uploadFileUrl) { _ in
            os_log("upload file url: \(uploadFileUrl?.absoluteString ?? "none")")
            if let uploadFileUrl = uploadFileUrl {
                do {
                    let fileUpload = try FileUploadModel(fileUrl: uploadFileUrl)
                    self.fileUpload = fileUpload
                    fileUpload.start()
                    fileUploadViewIsActive = true
                } catch NewFileUploaderError.InvalidEndpoint(let error) {
                    assertionFailure("invalid API endpoint: \(error)")
                } catch let error {
                    os_log("error starting file upload: \(error.localizedDescription)")
                }
            }
        }
        .onChange(of: fileUploadViewIsActive) { _ in
            if !fileUploadViewIsActive {
                fileUpload = nil
                uploadFileUrl = nil
            }
        }
    }

    var uploadFileButton: some View {
        Button(action: {
            uploadFilePickerPresented = true
        }) {
            Text("Send a file!")
        }
        .sheet(isPresented: $uploadFilePickerPresented) {
            uploadFilePicker
        }
    }

    var uploadFilePicker: some View {
        DocumentPicker(fileUrl: $uploadFileUrl, isPresented: $uploadFilePickerPresented)
            .ignoresSafeArea(.container, edges: .bottom)
    }
}
