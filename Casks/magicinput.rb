# frozen_string_literal: true

cask "magicinput" do
  version "0.1.1"
  sha256 "4496b4c93c6930dd51992f05596200cce5584bdfada07f6a7408da0dbac570df"

  url "https://github.com/betoxf/magicinput/releases/download/v#{version}/MagicInput-macOS.zip"
  name "MagicInput"
  desc "Share one keyboard and mouse with a Windows PC"
  homepage "https://github.com/betoxf/magicinput"

  depends_on macos: :ventura

  app "MagicInput.app"

  uninstall launchctl: "io.github.magicinput",
            quit:      "io.github.magicinput"

  zap trash: [
    "~/Library/Application Support/magicinput",
    "~/Library/LaunchAgents/io.github.magicinput.plist",
  ]
end
