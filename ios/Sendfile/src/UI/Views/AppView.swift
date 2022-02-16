import Foundation
import SwiftUI

struct AppView: View {
    @Environment(\.env) var env: AppEnvironment

    var body: some View {
        Text("Send a file!")
    }
}
