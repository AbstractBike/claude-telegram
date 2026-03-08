{
  description = "Claude Chat — Matrix/Claude multi-agent platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in {
        packages.claude-chat = rustPlatform.buildRustPackage {
          pname = "claude-chat";
          version = "0.5.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };

        packages.default = self.packages.${system}.claude-chat;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [ rustToolchain rust-analyzer bubblewrap age ];
          RUST_LOG = "claude_chat=debug";
        };
      }
    ) // {
      homeManagerModules.claude-chat = { config, lib, pkgs, ... }:
        let
          cfg = config.services.claude-chat;
          claudeChatPkg = self.packages.${pkgs.system}.claude-chat;
        in {
          options.services.claude-chat = {
            enable = lib.mkEnableOption "Claude Chat Matrix bot";
            configFile = lib.mkOption {
              type = lib.types.path;
              description = "Path to config.toml";
            };
            claudePath = lib.mkOption {
              type = lib.types.str;
              default = "claude";
              description = "Path to the claude CLI binary";
            };
            extraEnvironment = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [];
              description = "Extra environment variables for the service";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.claude-chat = {
              Unit = {
                Description = "Claude Chat Matrix Bot";
                After = [ "network.target" ];
              };
              Service = {
                ExecStart = "${claudeChatPkg}/bin/claude-chat";
                Restart = "on-failure";
                RestartSec = "10s";
                Environment = [
                  "CLAUDE_CHAT_CONFIG=${cfg.configFile}"
                  "CLAUDE_PATH=${cfg.claudePath}"
                  "RUST_LOG=claude_chat=info"
                ] ++ cfg.extraEnvironment;
              };
              Install.WantedBy = [ "default.target" ];
            };
          };
        };
    };
}
