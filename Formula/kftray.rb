require "language/node"

class Kftray < Formula
  desc "A tray app with Tauri and React"
  homepage "https://github.com/hcavarsan/kftray"
  version "HEAD"
  head "https://github.com/hcavarsan/kftray.git", branch: "main"
  license "MIT"

  depends_on "rust" => :build
  depends_on "node"



  def install
    ENV["CI"] = "true"
    system "npm", "install", *Language::Node.std_npm_install_args(libexec)
    system "npm", "install", "pnpm"
    system "npm", "install"

    if build.head?
      if OS.mac?
        system "npm", "run", "tauri", "build", "--", "-b", "app"
        app_bundle = "kftray.app"
        prefix.install "src-tauri/target/release/bundle/macos/#{app_bundle}"
        bin.install_symlink prefix/"kftray.app/Contents/MacOS/kftray" => "kftray"
        def caveats
          <<~EOS
            To link kftray to the Applications folder, run the following command:
              ln -s "#{prefix}/kftray.app" /Applications/kftray.app
          EOS
        end

      elsif OS.linux?
        system "npm", "run", "tauri", "build", "--", "-b", "AppImage"
        appimage = "src-tauri/target/release/bundle/linux/kftray.AppImage"
        bin.install appimage
        chmod 0755, bin/"kftray.AppImage"
        def caveats
          <<~EOS
            To integrate kftray into your system, run the following command:
              #{opt_bin}/kftray.AppImage --integrate
          EOS
        end
    end
  end
  end
end
