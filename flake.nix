{
  description = "oxabl-cache development shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          # The desktop app (`desktop/`) links xkbcommon and wayland, and draws through the
          # Vulkan loader; headless renders use Mesa's lavapipe, a software Vulkan driver.
          OXABL_DESKTOP_VULKAN_LIBS = pkgs.lib.makeLibraryPath [ pkgs.vulkan-loader ];
          OXABL_DESKTOP_LAVAPIPE_ICD = "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.uname.processor}.json";
          packages = [
            pkgs.pkg-config
            pkgs.libxkbcommon
            pkgs.wayland
            pkgs.cargo-zigbuild
            pkgs.zig
            pkgs.garage
            pkgs.versitygw
            pkgs.postgresql_18
            pkgs.libfaketime
            pkgs.cargo-fuzz
            pkgs.rustup
            pkgs.kubernetes-helm
            pkgs.kubeconform
            pkgs.opentofu
            pkgs.tflint
            pkgs.kind
            pkgs.kubectl
            pkgs.systemd
            pkgs.dbus
            pkgs.gnome-keyring
          ];
        };
      });
    };
}
