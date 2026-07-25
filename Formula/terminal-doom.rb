class TerminalDoom < Formula
  desc "Doom-inspired first-person shooter rendered entirely in a terminal"
  homepage "https://github.com/adamzafir/terminal-doom"
  url "https://github.com/adamzafir/terminal-doom/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "4f08b58213bd35a2c7f5ce588b2ad6d05aa897844d114322087cd8c6a2ba7b8b"
  license "MIT"
  head "https://github.com/adamzafir/terminal-doom.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "terminal-doom #{version}", shell_output("#{bin}/doom --version")
  end
end
