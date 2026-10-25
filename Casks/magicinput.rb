# frozen_string_literal: true

cask "magicinput" do
  version "0.6.2"
  sha256 "69514d9433a96199d912d1045b2de0e644e50c869233f8f555ddab8af7d130b7"

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
