class TerminalDoom < Formula
  desc "Doom-inspired first-person shooter rendered entirely in a terminal"
  homepage "https://github.com/adamzafir/terminal-doom"
  version "0.1.0"
  license "MIT"
  head "https://github.com/adamzafir/terminal-doom.git", branch: "main"

  depends_on :macos

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/adamzafir/terminal-doom/releases/download/v0.1.0/terminal-doom-aarch64-apple-darwin.tar.gz"
      sha256 "555180291b987984e5a2757137d3b18222ab9b66714406e48e187dd9edc5502d"
    else
      url "https://github.com/adamzafir/terminal-doom/releases/download/v0.1.0/terminal-doom-x86_64-apple-darwin.tar.gz"
      sha256 "e1e64ccf9d8851d28d4938457a7f54f200d8bcb9f1769be92ec276acb8d14b18"
    end
  end

  def install
    bin.install "doom"
  end

  test do
    assert_match "terminal-doom #{version}", shell_output("#{bin}/doom --version")
  end
end
