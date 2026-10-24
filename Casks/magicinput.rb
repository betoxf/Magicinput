# frozen_string_literal: true

cask "magicinput" do
  version "0.6.0"
  sha256 "74b0bd1e99c0062ac5a95b452c0d0197e29c8c6a3cd108fad213aa6d7c56cb53"

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
