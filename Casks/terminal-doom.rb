cask "terminal-doom" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.1"
  sha256 arm:   "f50e1fb081cc6991a45a26fd4b529f117196ecc6c993ea91ee623398ee5ac9e0",
         intel: "f6253dc6dae2abf043f5b4a51ed2342897cae2b7c69a0da84197129afb4c04eb"

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
