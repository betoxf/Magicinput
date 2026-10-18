# frozen_string_literal: true

cask "magicinput" do
  version "0.2.0"
  sha256 "c5fa05f20c1b8defbaa56e3b6ce515e8061afef8d3b8ba3e93d9303e09e219ba"

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
