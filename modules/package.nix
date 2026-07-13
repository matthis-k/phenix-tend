{ lib, ... }: {
  perSystem =
    { pkgs, ... }:
    let
      source = lib.cleanSource ../.;

      rustToolchain = [
        pkgs.cargo
        pkgs.rustc
        pkgs.rustfmt
        pkgs.clippy
      ];

      tendCliPkg = pkgs.rustPlatform.buildRustPackage {
        pname = "tend";
        version = "0.1.0";
        src = source;
        cargoLock.lockFile = ../Cargo.lock;
        cargoBuildFlags = "-p tend-cli";
        nativeBuildInputs = [ pkgs.git ];
      };

      cargoDeps = tendCliPkg.cargoDeps or (throw "cargoDeps not found");

      mkCargoCheck =
        name: cargoArgs: extraNativeBuildInputs:
        pkgs.runCommand name
          {
            nativeBuildInputs = extraNativeBuildInputs ++ [ pkgs.stdenv.cc ];
            inherit cargoDeps;
            src = source;
          }
          ''
            export HOME=$TMPDIR/home
            mkdir -p $HOME
            export CARGO_HOME=$TMPDIR/cargo
            export CARGO_TARGET_DIR=$TMPDIR/target
            mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

            cp -rT $src source
            chmod -R u+w source
            cd source

            mkdir -p .cargo
            cat > .cargo/config.toml <<EOF
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "${cargoDeps}"
            EOF

            ${cargoArgs}

            touch $out
          '';
    in
    {
      packages = {
        inherit tendCliPkg;
        tend = tendCliPkg;
        default = tendCliPkg;
      };

      checks = {
        cargo-check =
          mkCargoCheck "phenix-tend-cargo-check" "cargo check --workspace --all-targets"
            rustToolchain;

        cargo-test =
          mkCargoCheck "phenix-tend-cargo-test" "cargo test --workspace"
            [
              pkgs.cargo
              pkgs.rustc
              pkgs.git
            ];

        cargo-fmt =
          mkCargoCheck "phenix-tend-cargo-fmt" "cargo fmt --all --check" rustToolchain;

        cargo-clippy =
          mkCargoCheck "phenix-tend-cargo-clippy"
            "cargo clippy --quiet --workspace --all-targets -- -D warnings"
            rustToolchain;

        tend-gate =
          pkgs.runCommand "phenix-tend-tend-gate"
            {
              nativeBuildInputs = [
                tendCliPkg
                pkgs.git
                pkgs.nix
                pkgs.nixfmt
                pkgs.statix
                pkgs.deadnix
                pkgs.stdenv.cc
              ]
              ++ rustToolchain;
              inherit cargoDeps;
              src = source;
            }
            ''
              export HOME=$TMPDIR/home
              mkdir -p $HOME
              export NIX_STATE_DIR=$TMPDIR/nix-state
              mkdir -p $NIX_STATE_DIR/profiles
              export NIX_PATH=nixpkgs=${pkgs.path}
              export CARGO_HOME=$TMPDIR/cargo
              export CARGO_TARGET_DIR=$TMPDIR/target
              mkdir -p $CARGO_HOME $CARGO_TARGET_DIR

              cp -rT $src source
              chmod -R u+w source
              cd source

              mkdir -p .cargo
              cat > .cargo/config.toml <<EOF
              [source.crates-io]
              replace-with = "vendored-sources"

              [source.vendored-sources]
              directory = "${cargoDeps}"
              EOF

              git init --quiet
              git add -A

              tend check --profile nix-check --context nix-sandbox

              touch $out
            '';
      };

      apps = {
        tend = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
        default = {
          type = "app";
          program = "${tendCliPkg}/bin/tend";
        };
      };

      devShells.default = pkgs.mkShell {
        name = "phenix-tend-dev";
        packages = [
          pkgs.rust-analyzer
          pkgs.git
          pkgs.nix
          tendCliPkg
        ]
        ++ rustToolchain;
        shellHook = ''
          echo "phenix-tend dev shell"
          echo "  cargo: $(cargo --version 2>/dev/null || echo '?')"
          echo "  rustc: $(rustc --version 2>/dev/null || echo '?')"
          echo "  tend:  $(tend --version 2>/dev/null || echo '?')"
        '';
      };
    };
}
