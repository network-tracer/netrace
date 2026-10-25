import SwiftUI

@main
struct NetraceApp: App {
    @AppStorage(AppearanceSetting.defaultsKey) private var appearance = AppearanceSetting.system

    var body: some Scene {
        DocumentGroup(newDocument: { ProjectDocument() }) { file in
            ProjectWindow(document: file.document)
                .onAppear { appearance.apply() }
        }
        .defaultSize(width: 1280, height: 800)
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands {
            AppearanceCommands()
            WorkspaceCommands()
        }
    }
}
