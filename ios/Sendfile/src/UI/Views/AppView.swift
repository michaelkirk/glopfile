import Foundation
import SendfileRustFFI
import SwiftUI
import os

struct AppView: View {
    @Environment(\.env) var env: AppEnvironment

    @EnvironmentObject var sceneDelegate: SendfileSceneDelegate

    @State var uploadFileUrl: URL?
    @State var downloadUrl: URL?
    @State var uploadFilePickerPresented = false
    @State var fileUploadViewIsActive = false
    @State var fileDownloadViewIsActive = false

    var body: some View {
        NavigationView {
            ZStack {
                uploadView
                NavigationLink(
                    destination: FileUploadView(fileUrl: $uploadFileUrl),
                    isActive: $fileUploadViewIsActive
                ) {
                    EmptyView()
                }
                NavigationLink(
                    destination: FileDownloadView(url: $downloadUrl),
                    isActive: $fileDownloadViewIsActive
                ) {
                    EmptyView()
                }
            }
        }
    }

    var uploadView: some View {
        VStack(alignment: .center) {
            uploadFileButton
        }
        .onChange(of: uploadFileUrl) { _ in
            if let _ = uploadFileUrl {
                fileUploadViewIsActive = true
            }
        }
        .onChange(of: fileUploadViewIsActive) { _ in
            if !fileUploadViewIsActive {
                uploadFileUrl = nil
            }
        }
        .onOpenURL { url in
            openUrl(url: url)
        }
        .onChange(of: sceneDelegate.openedUrl) { _ in
            if let openedUrl = sceneDelegate.openedUrl {
                openUrl(url: openedUrl)
            }
        }
        .onChange(of: fileDownloadViewIsActive) { _ in
            if !fileDownloadViewIsActive {
                downloadUrl = nil
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

    func openUrl(url: URL) {
        var censoredUrl = URLComponents(url: url, resolvingAgainstBaseURL: false)
        let censoredFragment = censoredUrl?.fragment
        censoredUrl?.fragment = censoredFragment?
            .replacingOccurrences(of: "[^~]", with: "X", options: .regularExpression, range: nil)
        os_log("opening url: \(censoredUrl?.string ?? "nil")")

        downloadUrl = url
        fileDownloadViewIsActive = true
        uploadFilePickerPresented = false
    }
}
