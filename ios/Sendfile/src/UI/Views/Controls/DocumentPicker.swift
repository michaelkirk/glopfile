import Foundation
import SwiftUI

struct DocumentPicker: UIViewControllerRepresentable {
    @Binding var fileUrl: URL?
    @Binding var isPresented: Bool

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker: UIDocumentPickerViewController
        if #available(iOS 14, *) {
            picker = UIDocumentPickerViewController(forOpeningContentTypes: [.data])
        } else {
            picker = UIDocumentPickerViewController(documentTypes: ["public.data"], in: .import)
        }

        picker.delegate = context.coordinator

        return picker
    }

    func updateUIViewController(_ picker: UIDocumentPickerViewController, context: Context) {
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    class Coordinator: NSObject, UIDocumentPickerDelegate, UINavigationControllerDelegate {
        let picker: DocumentPicker

        init(_ picker: DocumentPicker) {
            self.picker = picker
        }

        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            picker.fileUrl = urls.first
        }

        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
            picker.isPresented = false
        }
    }
}
