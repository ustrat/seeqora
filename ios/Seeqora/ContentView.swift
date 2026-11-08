import SwiftUI

/// Phase 0 W1 shell: proves the Rust core links and answers over UniFFI.
/// `coreVersion()` comes from the generated bindings in Generated/.
struct ContentView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("Seeqora")
                .font(.largeTitle.bold())
            Text("Core \(coreVersion())")
                .font(.footnote.monospaced())
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}

#Preview {
    ContentView()
}
