# Image context budget

Read this before repeated screenshots or graphical-session loops in any project, and after a request-size failure caused by images.

## Rules

- Capture to exact local files and verify success and freshness before inspection, because a failed capture can leave an older file that looks current.
- Use application state, DOM or accessibility trees, logs, OCR boxes or pixel measurements in routine capture loops, because each attached image stays in conversation history and the total grows every turn.
- Keep originals and recordings on disk and retrieve history as text, because a recording belongs in an artifact, not the prompt.
- Name a visual decision that text cannot settle before using view_image or image output, because most navigation needs only coordinates and text.
- Crop the relevant region to a small JPEG and check its encoded size, because per-image resizing alone did not stop cumulative growth.
- Allow at most 2 previews per context, 128 KiB base64 each and 256 KiB total, counting child and tool duplicates at `4 * ceil(file_bytes / 3)`, because that leaves room for the rest of the request under the upstream limit.
- Use text only when prior image volume is unknown, because the budget cannot be checked without it.
- Keep user-requested image deliverables separate from development inspection, because the budget limits inspection, not visual artifacts Tom asked for.
- Switch an oversized context immediately to text-only paths and conclusions, and resume previews only after the runtime confirms prior image content was removed, because even a tiny thumbnail adds to an already rejected history.
- Never retry the oversized request unchanged or ask Tom to repeat work, because the same payload fails again.

## Why these limits

Repeated Warcraft development sessions exceeded the upstream websocket request limit: requests of 28,330,279 and 17,679,086 bytes were rejected against a 15,728,640-byte limit. The second failure followed a promise of smaller captures and then repeated 960-pixel previews. The two-preview, 256-KiB budget is a conservative inspection budget, not a measured provider limit.

Matching case: two-client lobby navigation uses fresh disk captures, OCR text and boxes, and short factual updates; animation review may use one cropped preview inside the budget and outside a rejected context. Nonmatching case: an exported illustration or figure Tom asked for is a deliverable and stays allowed.

Nothing here is mechanically enforced, and no tool proves prior images were removed; revise these limits only with a new measured failure.
