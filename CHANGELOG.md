## [0.3.6] - 2026-10-30

### Features

- feat(login): fall back to next card when one is exhausted d18236a 

### Bug Fixes

- fix(reward): prefer monthly card when picking active card 98b21bd 

### Miscellaneous

- ci(release): add windows x86_64 target to release build matrix 416fc3f 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.5...v0.3.6
## [0.3.5] - 2026-10-27

### Features

- feat: add status logging for login attempts and success 8573d96 

### Bug Fixes

- fix: treat 204 No Content response as already online 0f004ec 

### Miscellaneous

- chore: release v0.3.4 cbbb4e1 
- ci: add binary building and checksum generation to release workflow b651b48 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.4...v0.3.5
## [0.3.4] - 2026-10-25

### Features

- feat(systemd): add auto login service and daily reward timer and service 4ad2f6a 

### Miscellaneous

- ci: add GitHub Actions workflows for CI and release ec98f3e 
- chore: release v0.3.4 547f21d 
- ci: install dbus deps and apply rustfmt 9cb8572 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.3...v0.3.4
## [0.3.3] - 2026-10-25

### Features

- feat(auto): keep running and re-login when offline 42d9e29 

### Documentation

- docs(readme): document auto mode interval and retry defaults 47c3fe2 

### Miscellaneous

- chore: release v0.3.3 1c11f9b 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.2...v0.3.3
## [0.3.2] - 2026-10-25

### Features

- feat(reward): fallback to file-based storage for credentials 06fd773 
- feat: add `--auto` and `--retry-after` flags for automatic login 2934164 

### Miscellaneous

- chore: release v0.3.2 0f57c6a 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.1...v0.3.2
## [0.3.1] - 2026-09-26

### Features

- feat: add no-logs login option 060640a 

### Miscellaneous

- chore: bump version to 0.3.1 35698b6 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.3.0...v0.3.1
## [0.3.0] - 2026-09-23

### Features

- feat(cli): add card usage subcommand 3b85398 
- feat(usage): add card usage command de7d6e3 

### Other

- build: add time dependency 39fdfd7 

### Refactor

- refactor(reward): extract reusable authentication session 40bcbbc 

### Miscellaneous

- chore: bump version to 0.3.0 bf9fb4f 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.2.0...v0.3.0
## [0.2.0] - 2026-09-21

### Features

- feat(cli): add shop rewards subcommand 4d2f4ca 
- feat(shop): implement reward claim c1a110d 

### Miscellaneous

- chore: ignore .env b96d33e 
- chore: bump wi-mesh-login version to 0.2.0 6e3b4ba 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.1.1...v0.2.0
## [0.1.1] - 2026-09-20

### Bug Fixes

- fix(cli): correct login command description wording a176b0c 
- fix(login): report traffic limit errors clearly 001d958 

### Miscellaneous

- chore: update project description to cli 1345efd 
- chore: bump wi-mesh-login version to 0.1.1 9b39d76 


#### Full Changelog: https://github.com/ak4duy/wi-mesh-login/compare/v0.1.0...v0.1.1
## [0.1.0] - 2026-09-17

### Features

- feat: add MIT License afbd229 

### Refactor

- refactor: rewrite in Rust with CLI support 094c24a 

### Documentation

- docs: better formatting c6f506b 
- docs: add note a6ef67d 

