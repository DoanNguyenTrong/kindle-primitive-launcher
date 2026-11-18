# Settings design

The launcher's Settings follow Apple's grouped-list pattern, adapted to a grayscale e-ink screen. Apple's
Liquid Glass material (translucency and depth through blur) cannot be shown on e-ink, so its ideas are
kept in other forms.

## Sources

- Apple's Human Interface Guidelines pages (Settings, Designing for iOS) could not be read in full while this was
  written; only their titles were returned. The guidance below uses what those pages cover in general.
- Liquid Glass summaries used: [Create with Swift, "Liquid Glass: Redefining design through Hierarchy,
  Harmony and Consistency"](https://www.createwithswift.com/liquid-glass-redefining-design-through-hierarchy-harmony-and-consistency/),
  and [superapp.dev, "How to Design a Liquid Glass App for iOS 26 and iOS 27"](https://superapp.dev/blog/how-to-design-a-liquid-glass-app-for-ios).
  These are secondary; check the Apple pages before relying on a detail.

## Rules we apply

| Apple idea | On the Kindle |
|---|---|
| Grouped lists: related settings in one card, with a header | Each page is sections: a header, one card, a footer |
| Footers explain a setting, briefly | One or two lines under a card; nothing that repeats the label |
| Switches for on and off | A switch on the right of the row, black when on; no "Turn on" buttons |
| Disclosure chevrons for pages | A "›" on each page row of the Settings list; the value sits under the title |
| Checkmarks for one choice of many | The connected network carries "Connected" and a check mark |
| Content leads over chrome | Only a title and one Back or Done button; no bars or boxes around the content |
| Hierarchy by size and weight | Large title; bold row titles; grey value text; small grey footers |
| Touch targets of about 44 points | Rows are 120 px tall; switches are 112 × 64 px; round buttons 72 px |
| Standard, predictable navigation | Back returns one level; Done returns to the grid; the back key does the same |
| Plain labels | "Wi-Fi", "Dark mode", "Start with the Kindle", not technical names |

Liquid Glass ideas that cannot be shown here: translucent bars, depth from blur, and motion. Depth is shown by
the card outlines and the light hairlines between rows.

## Wi-Fi

The Wi-Fi page lists the network the Kindle is on (name, signal, secured) and the networks it can see, strongest
first, with a lock for secured networks and "Saved" for networks the Kindle already knows. Scan runs when the
page opens and when Scan is tapped. Joining a network is the next step (M5).
