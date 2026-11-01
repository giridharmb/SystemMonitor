# SystemMonitor app icon

`AppIcon.png` is the 1024 × 1024 RGBA master, generated with the built-in image-generation tool and resized using Apple's `sips`. Transparency around the rounded tile is preserved. `AppIcon.icns` includes the ten standard macOS representations, from 16 pixels to 512 pixels at 2×.

Rebuild the checked-in `.icns` after replacing the master:

```sh
./scripts/build-icon.sh
```

The script uses `sips` and `iconutil`, then decodes the result and checks all ten representations. Normal builds use the checked-in file and do not run image generation. The command-line bundler copies it to `Contents/Resources`; Xcode copies it through its Resources phase. Both Info.plists identify `AppIcon` with `CFBundleIconFile`. Each build signs the icon as part of the app's resources.

The updated signed-only DMG is under `dist/with-icon/`; the earlier release is preserved. This icon adds no runtime rendering work or dependencies. Older Finder/Dock icon caches may continue to show an earlier icon for an already-running app; quit and reopen the updated app.

## Generation prompt

Use case: logo-brand. Asset type: production macOS application icon for SystemMonitor, 1024 x 1024 square image with real transparency outside the icon. Create one polished icon, no presentation board. A gently rounded square midnight navy ceramic/glass tile, front-on, centered, occupying about 88% of the canvas, with generous transparent margins and soft restrained shadow. On the tile: one bold, beautifully legible luminous pulse waveform in cyan transitioning to mint, rising into a single strong peak and returning to baseline, with rounded stroke caps; a subtle ring around the waveform suggests a precision system gauge. Subtle dimensional bevel and controlled cool edge highlights, elegant professional Mac utility aesthetic. High contrast simple silhouette recognizable at 16px, restrained depth and shading, no tiny grid details. Inspired by this app's dark monitoring dashboards, graphs and overall system health. No lettering, numerals, words, logos from existing brands, watermarks, computer monitor illustration, extra objects, mockup scenery, or outer colored background. Keep the corners and exterior truly transparent.
