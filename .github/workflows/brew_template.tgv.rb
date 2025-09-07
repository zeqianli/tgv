class Tgv < Formula
    desc "Terminal Genome Viewer"
    homepage "https://github.com/zeqianli/tgv/"
    version "__VERSION__"

    if OS.mac?
        url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-aarch64-apple-darwin.tar.gz"
        sha256 "__MACOS_SHA256__"
    elsif OS.linux?
        url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-x86_64-linux-musl.tar.gz"
        sha256 "__LINUX_SHA256__"
    end
  
    def install
      bin.install "tgv"
    end

    test do
        system "#{bin}/tgv", "--version"
    end
  end
