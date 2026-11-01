# Nix. One prefix gives three packages:
#   nix         a flake and package.nix with the files: `nix build`, `nix run`, `callPackage`, an overlay
#   nix-cache   a binary cache holding the files as one store path: `nix copy --from file://CACHE STOREPATH`
#   nix-export  the same store path as one file for `nix-store --import`
#
# Nix has no distro packages, so a library is a nixpkgs attribute: `inputs` lists what the programs link against, and
# autoPatchelfHook points them at it. The options are in docs/formats.md.

package(
    "nix",
    name = "hello",
    version = "1.0.0",
    description = "A small program",
    homepage = "https://example.org/hello",
    license = "MIT",
    options = {
        "inputs": ["zlib", "stdenv.cc.cc.lib"],
        "main_program": "hello",
    },
)

for format in ["nix-cache", "nix-export"]:
    package(
        format,
        name = "hello",
        version = "1.0.0",
        description = "A small program",
    )
