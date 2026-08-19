cask "simpleedit" do
  version "0.7.1"
  sha256 "e0469c94849ebbb2ecdc15e1e91394cf62eb4be253e329754c85876ff1fed2cc"

  url "https://github.com/simple-edit-app/simpleedit/releases/download/v#{version}/simpleedit-v#{version}-macos-aarch64.zip",
      verified: "github.com/simple-edit-app/simpleedit/"
  name "SimpleEdit"
  desc "Fast, stable, cross-platform text editor with syntax highlighting"
  homepage "https://simple-edit-app.github.io/simpleedit/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "SimpleEdit.app"
  binary "#{appdir}/SimpleEdit.app/Contents/MacOS/simpleedit"

  zap trash: [
    "~/Library/Application Support/simpleedit",
    "~/Library/Preferences/app.simpleedit.SimpleEdit.plist",
    "~/Library/Saved Application State/app.simpleedit.SimpleEdit.savedState",
  ]
end
