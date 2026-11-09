# Testing a release by hand

Automated checks cover the engine, the scene files and every example on macOS and Windows
(see [AGENTS.md](../AGENTS.md)). They do not cover the app's window: menus, dialogs, the
settings panel and the updater. Before a release, run through this list in the built app, the
one installed from the release's DMG or setup program, not `npm run tauri dev`. It takes about
15 minutes. Note anything wrong, with the step's number.

## 1. Install and start

1. Install from the draft release's download. The first launch asks to open an unsigned app;
   the release notes say how. It opens without an error.
2. **Far Ray Tracer > About Far Ray Tracer** (Help > About on Windows) shows the version being
   tested, the techniques, the author and the GitHub link.
3. **Check for Updates…** says the app is up to date, or offers a newer version.

## 2. Open and render

4. **File > Open Example > Showcase.** It renders to the end; the status bar says "Denoised
   by Open Image Denoise".
5. Drag the image to orbit, Shift-drag to pan, scroll to move closer: a quick preview follows,
   and the full render starts when you let go.
6. Click the crop button and draw a rectangle: only that part renders again. × clears it.
7. **File > Export Image…** (`Cmd/Ctrl+E`) saves a PNG that opens in another app.

## 3. The settings panel

8. Each icon on the left opens its group; sections fold and stay folded after reopening the
   app.
9. Search "exposure": only that setting shows. Esc clears the search.
10. Drag a number sideways, then click it and type a value past its usual range: the field
    turns amber and the render follows. Roughness typed as 5 becomes 1.
11. **Image > Tone mapping > AgX Punchy**, then **Camera > Depth of field** on with
    **Shape > 6 blades**: the image changes as expected.

## 4. Light, sky and materials

12. **Light > Environment**, menu in the header: **Physical sky**. Move **Sun elevation** from
    60 down to 2: the light turns warm and the shadows long. **Sun from > Time of day** and
    change the hour.
13. **Materials**: pick a material, change its preset and color: the ball preview and the
    render follow. The expand button shows the large ball, which orbits.
14. Open **Techniques > Spotlights**, **Smoke** and **Models**: each renders.

## 5. Saving

15. With edits made, the save button shows a dot. **File > Save Scene** on an example asks
    where to save (examples are never overwritten). Save it in Documents.
16. Close the app and open the saved file with **Open Scene…**: the camera, sky, material
    edits, tone mapping and exposure are as they were saved.
17. Change something, then open another example: the app asks to Save, Don't Save or Cancel.
    Cancel keeps the scene; Don't Save opens the other one.
18. Change something and quit (`Cmd+Q` or closing the window): the same question. Cancel keeps
    the app open.
19. Open an OBJ or glTF file directly (**Open Scene…**, any `.obj`, `.gltf` or `.glb`): it
    appears in a small studio and renders.

## 6. Updating

20. With an older version installed, **Check for Updates…** offers the new one; **Install**
    downloads it and the app restarts as the new version.

## Afterwards

Report the failing steps by number. A release goes out when every step passes, or what fails
is understood and listed in the release notes.
