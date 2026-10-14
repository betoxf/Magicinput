# frozen_string_literal: true

cask "magicinput" do
  version "0.1.0"
  sha256 "SHA256_PLACEHOLDER"

  url "https://github.com/betoxf/magicinput/releases/download/v#{version}/MagicInput-macOS.zip"
  name "MagicInput"
  desc "Share one keyboard and mouse between a Mac and a Windows PC"
  homepage "https://github.com/betoxf/magicinput"

  depends_on macos: ">= :ventura"

  app "MagicInput.app"

  uninstall launchctl: "io.github.magicinput",
            quit:      "io.github.magicinput"

  zap trash: [
    "~/Library/Application Support/magicinput",
    "~/Library/LaunchAgents/io.github.magicinput.plist",
  ]
end
