# frozen_string_literal: true

cask "magicinput" do
  version "0.5.0"
  sha256 "0284b4bb3ea89a78e48361cc27a2745a803db1d222dc83796a2b31db61a62462"

  url "https://github.com/betoxf/Magicinput/releases/download/v#{version}/MagicInput-macOS.zip"
  name "MagicInput"
  desc "Share one keyboard and mouse with a Windows PC"
  homepage "https://github.com/betoxf/Magicinput"

  depends_on macos: :ventura

  app "MagicInput.app"
  binary "#{appdir}/MagicInput.app/Contents/MacOS/magicinput"

  uninstall launchctl: "io.github.magicinput",
            quit:      "io.github.magicinput"

  zap trash: [
    "~/Library/Application Support/magicinput",
    "~/Library/LaunchAgents/io.github.magicinput.plist",
  ]
end
