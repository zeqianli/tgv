class Tgv < Formula
    desc "Terminal Genome Viewer"
    homepage "https://github.com/zeqianli/tgv/"
    version "__VERSION__"

    if OS.mac?
        url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-aarch64-apple-darwin.tar.gz"
        sha256 "__MACOS_SHA256__"

        resource "tgv-gui" do
            url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-gui-aarch64-apple-darwin.tar.gz"
            sha256 "__GUI_MACOS_SHA256__"
        end
    elsif OS.linux?
        url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-x86_64-linux-musl.tar.gz"
        sha256 "__LINUX_SHA256__"

        resource "tgv-gui" do
            url "https://github.com/zeqianli/tgv/releases/download/v#{version}/tgv-gui-x86_64-linux-gnu.tar.gz"
            sha256 "__GUI_LINUX_SHA256__"
        end
    end
  
    def install
      bin.install "tgv"
      resource("tgv-gui").stage { bin.install "tgv-gui" }
    end

    def caveats
        on_linux do
            <<~EOS
              tgv-gui needs Wayland or X11, a Vulkan-capable graphics driver, and the
              fontconfig, freetype, and xkbcommon libraries from your system. It also
              uses the Liberation Mono font (fonts-liberation on Debian and Ubuntu).
            EOS
        end
    end

    test do
        system "#{bin}/tgv", "--version"
        system "#{bin}/tgv-gui", "--version"
    end
  end
