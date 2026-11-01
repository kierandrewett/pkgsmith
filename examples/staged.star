# Packages of a program that was built on macOS or Windows. pkgsmith does not build for those systems. Build the program
# there, put its files in a prefix, and run pkgsmith on any machine that can read the prefix.
#
#   cmake --install build --prefix prefix/hello        (on the Mac or on Windows)
#   pkgsmith build -f examples/staged.star --prefix prefix -F dmg
#
# The prefix holds the program in hello/, and include() keeps only that folder. The dmg and the installers copy it into
# their own folder. `arch` is the
# architecture of the files: pass --build-arg ARCH=amd64 for an Intel build.

include("/hello/**")

common = {
    "name": "hello",
    "version": arg("VERSION", "1.0.0"),
    "description": "A small program",
    "arch": arg("ARCH", host()["arch"]),
}

package("dmg", distro = "macos", options = {"volume_name": "hello"}, **common)
package("msi", distro = "windows", options = {"prefix": "/hello", "manufacturer": "Example"}, **common)
package("nsis", distro = "windows", options = {"prefix": "/hello"}, **common)
