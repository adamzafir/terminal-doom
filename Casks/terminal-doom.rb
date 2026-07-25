cask "terminal-doom" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.0"
  sha256 arm:   "555180291b987984e5a2757137d3b18222ab9b66714406e48e187dd9edc5502d",
         intel: "e1e64ccf9d8851d28d4938457a7f54f200d8bcb9f1769be92ec276acb8d14b18"

  url "https://github.com/adamzafir/terminal-doom/releases/download/v#{version}/terminal-doom-#{arch}-apple-darwin.tar.gz"
  name "Terminal Doom"
  desc "Doom-inspired first-person shooter rendered entirely in a terminal"
  homepage "https://github.com/adamzafir/terminal-doom"

  binary "doom"
end
