# frozen_string_literal: true

cask "magicinput" do
  version "0.3.1"
  sha256 "90e0c25bf6e732c29b72d81f1b9d8a285f769443aa7fcf7ae9e794f4c614de43"

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
