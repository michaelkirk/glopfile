import SwiftUI

@main
struct SendfileAppMain {
    public static func main() {
        setRustLogger()
        SendfileApp.main()
    }
}

struct SendfileApp: App {
    @UIApplicationDelegateAdaptor var delegate: SendfileAppDelegate

    var body: some Scene {
        WindowGroup {
            AppView()
        }
    }
}

class SendfileAppDelegate: NSObject, UIApplicationDelegate, ObservableObject {
    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let sceneConfig = UISceneConfiguration(name: nil, sessionRole: connectingSceneSession.role)
        sceneConfig.delegateClass = SendfileSceneDelegate.self
        return sceneConfig
    }
}

class SendfileSceneDelegate: NSObject, UIWindowSceneDelegate, ObservableObject {
    @Published var openedUrl: URL?

    func scene(_ scene: UIScene, continue userActivity: NSUserActivity) {
        if userActivity.activityType == NSUserActivityTypeBrowsingWeb,
            let url = userActivity.webpageURL
        {
            openedUrl = url
        }
    }
}
