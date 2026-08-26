# Branding asset provenance

`app-icon-source.png` is the source raster for the generated Tauri icon set in
`src-tauri/icons/`. It was created for Coding Agent Monitor on 2026-08-26 with
OpenAI's built-in image generation tool and then resized by `pnpm tauri icon`.

Final prompt:

> Use case: precise-object-edit. Asset type: Windows desktop app and 16x16
> system-tray icon. Simplify the draft into one compact glyph combining a
> terminal chevron with exactly three usage bars. Use a centered, strong
> silhouette, transparent background, deep indigo and warm terracotta, no text,
> trademarks, watermark, mockup, or resemblance to an existing technology logo.

The icon is made available under the repository's MIT License. It intentionally
replaces the Tauri starter icon and must not be presented as a logo of Tauri,
OpenAI, Anthropic, or any other third party.

Regenerate platform files after changing the source:

```powershell
pnpm tauri icon assets/branding/app-icon-source.png
```
