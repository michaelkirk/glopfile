import Foundation
import SwiftUI
import os

struct DocumentViewer: UIViewControllerRepresentable {
    let fileUrl: URL
    @Binding var isPresented: Bool

    func makeUIViewController(context: Context) -> Controller {
        return Controller(fileUrl: fileUrl, isPresented: $isPresented)
    }

    func updateUIViewController(_ controller: Controller, context: Context) {
        controller.fileUrl = fileUrl
        controller.isPresented = $isPresented
        controller.update()
    }

    class Controller: UIViewController, UIDocumentInteractionControllerDelegate {
        var fileUrl: URL
        var isPresented: Binding<Bool>

        private var presentedDocumentInteractionController: UIDocumentInteractionController?

        init(fileUrl: URL, isPresented: Binding<Bool>) {
            self.fileUrl = fileUrl
            self.isPresented = isPresented
            super.init(nibName: nil, bundle: nil)
        }

        required init?(coder: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        override func didMove(toParent parent: UIViewController?) {
            super.didMove(toParent: parent)
            update()
        }

        func update() {
            if let controller = presentedDocumentInteractionController {
                if controller.url != fileUrl {
                    controller.dismissPreview(animated: true)
                    presentedDocumentInteractionController = nil
                }
            }

            let isPresented = presentedDocumentInteractionController != nil
            if isPresented != self.isPresented.wrappedValue {
                if !isPresented {
                    if parent != nil {
                        let controller = UIDocumentInteractionController(url: fileUrl)
                        controller.delegate = self
                        if controller.presentPreview(animated: true) {
                            presentedDocumentInteractionController = controller
                        }
                    }
                } else {
                    presentedDocumentInteractionController?.dismissPreview(animated: true)
                    presentedDocumentInteractionController = nil
                }
            }
        }

        func documentInteractionControllerViewControllerForPreview(_ controller: UIDocumentInteractionController) -> UIViewController {
            return self
        }

        func documentInteractionControllerDidEndPreview(_ viewer: UIDocumentInteractionController) {
            presentedDocumentInteractionController = nil
            viewer.delegate = nil
            isPresented.wrappedValue = false
        }
    }
}
