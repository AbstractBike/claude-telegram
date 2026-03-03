{
  description = "Claude Telegram Bot — Nix flake with Home Manager module";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        pythonEnv = pkgs.python312.withPackages (ps: [
          ps.python-telegram-bot
          ps.pytest
          ps.pytest-asyncio
          ps.pytest-mock
        ]);
      in {
        packages.claude-telegram = pkgs.writeShellScriptBin "claude-telegram" ''
          export PYTHONPATH=${self}/
          exec ${pythonEnv}/bin/python -m bot.main
        '';

        devShells.default = pkgs.mkShell {
          packages = [ pythonEnv pkgs.nodejs ];
        };
      }
    ) // {
      homeManagerModules.claude-telegram = { config, lib, pkgs, ... }:
        let
          cfg = config.services.claude-telegram;
          pythonEnv = pkgs.python312.withPackages (ps: [
            ps.python-telegram-bot
          ]);
          botScript = pkgs.writeShellScriptBin "claude-telegram" ''
            export PYTHONPATH=${self}/
            export TELEGRAM_TOKEN=$(cat ${cfg.tokenFile})
            export ALLOWED_USERS="${lib.concatStringsSep "," cfg.allowedUsers}"
            export CLAUDE_PATH="${cfg.claudePath}"
            exec ${pythonEnv}/bin/python -m bot.main
          '';
        in {
          options.services.claude-telegram = {
            enable = lib.mkEnableOption "Claude Telegram bot";
            tokenFile = lib.mkOption {
              type = lib.types.path;
              description = "Path to file containing the Telegram bot token (chmod 600)";
            };
            allowedUsers = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [];
              description = "List of Telegram @usernames allowed to use the bot (empty = open)";
            };
            claudePath = lib.mkOption {
              type = lib.types.str;
              default = "claude";
              description = "Full path to the claude binary (required when systemd PATH does not include it)";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.claude-telegram = {
              Unit = {
                Description = "Claude Telegram Bot";
                After = [ "network.target" ];
              };
              Service = {
                ExecStart = "${botScript}/bin/claude-telegram";
                Restart = "on-failure";
                RestartSec = "10s";
              };
              Install.WantedBy = [ "default.target" ];
            };
          };
        };
    };
}
