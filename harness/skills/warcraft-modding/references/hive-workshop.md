# Hive Workshop releases

Read this to publish a map release on Hive Workshop: replace the map file, update the page text and announce it in the thread.

## Where things are

- Smashcraft's bundle thread is https://www.hiveworkshop.com/threads/smashcraft.375492/ and its page editor is https://www.hiveworkshop.com/bundles/smashcraft.375492/edit (title, description, images, tags).
- The map file lives in a resource inside the bundle, edited at `/resources/<slug>.<id>/edit` (Smashcraft: id 170520); find the current slug through the thread page's `Edit` link under the resource, because Hive renames the slug after the uploaded map's title.
- The page text is smashcraft:docs/hive-post.md; `bun run hive:post` from smashcraft's `ts/` prints it as Hive BBCode.

## Procedure

- Work in `automation-chrome` on the private desktop, never Tom's own Chrome, because Tom asked for releases without taking over his screen; its profile stays signed in to the release account (south:agents/skills/battlenet-accounts/) with Stay logged in.
- When the private desktop defers with DEFER_GAMING, ask Tom before closing Battle.net, then close it with `~/code/north/main/bin/tom-warcraft-close`, because the capacity guard counts his launcher as gaming.
- When navigation hangs on Unlock Login Keyring or Hive shows Log in, open a viewer with `north run-bounded 30m -- remmina -c vnc://127.0.0.1:PORT` (the run's `port` file) and let Tom type the keyring password and Hive sign-in himself, because both are his credentials; close the viewer afterwards.
- Replace the map on the resource edit page: click the dropzone's Delete, then `{"action":"upload","selector":".upload-map-container input[type=file]","path":"/absolute/Smashcraft X.Y.Z.w3x"}`, wait for the green check and the new file name, keep Send alert and Bump ticked, and press Post resource.
- Update the description on the bundle edit page: press Toggle BB code (`#xfBbCode-1`), fill `textarea[name=message]` with the generated BBCode and press Post bundle; diff the live text against the previous release's file first, because Tom may have edited the page by hand.
- Announce in the thread: click the quick-reply editor, toggle BB code, fill `form[action$="add-reply"] textarea[name=message]` with a short "Smashcraft X.Y.Z is out" post and the New in list, and press Post reply.
- Verify by reading the thread: the resource link carries the new version slug and size, the page shows the new title, and the reply is the last post; then close every tab you opened and leave the browser service running, because a stopped service relocks the keyring and Tom must unlock it again.
