cask "terminal-doom" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.2"
  sha256 arm:   "618d84456ea46c37c39987de91d647034c6375b11023db0575e69d2cb7111507",
         intel: "bd555f8973e3bf7fc2b4f400c4fa33daaa776902b7336fd113737185ca4afd92"

  url "https://github.com/adamzafir/terminal-doom/releases/download/v#{version}/terminal-doom-#{arch}-apple-darwin.tar.gz"
  name "Terminal Doom"
  desc "Doom-inspired first-person shooter rendered entirely in a terminal"
  homepage "https://github.com/adamzafir/terminal-doom"

  binary "doom"

  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-d", "com.apple.quarantine", "#{staged_path}/doom"]
  end
end
