# Convert the existing Apple membership to LEANZERO S.R.L.

Prepared 26 October 2026. The owner chose conversion before publication. This is the request preparation record; it does not record a submitted request, completed conversion or accepted legal declaration.

## Existing account

Keep the existing Apple Developer Program membership and team `ZZ8MTZ6NRZ`. App Store Connect's provider response currently identifies `developerProgram=INDIVIDUAL`, `entityType=INDIVIDUAL`, seller `Mihai Perdum`, provider `6213b7a9-0a22-4f2b-a0da-524be5e2a657`, with the user's `LEGAL`, `CIPS` and `ADMIN` roles. The source is the read-only provider receipt `/tmp/goose-appstore-providers.json`, captured during this release preparation. The same team already serves Goose's Mac signing/notarization and the iOS archive/export work.

The provider response also contains `isSignupComplete=false`. Do not infer from that field alone which screen or agreement remains: inspect the authenticated Apple account. A signed/exported iOS archive establishes signing capability, not the desired organization seller identity or publication readiness.

Apple's official conversion procedure is an account-update request by the Account Holder. It specifically requires the requester to be the organization's founder/cofounder and to provide its D-U-N-S number; Apple may request business documents. This is the existing-membership conversion route, not a new enrollment or app transfer. Changing the personal Apple Account display name does not change an individual membership's App Store seller name. [Apple account-update requirements](https://developer.apple.com/help/account/membership/updating-your-account-information/).

Open [Apple's individual-to-organization request](https://developer.apple.com/contact/request/migrate-individual-account) while signed in as the existing Account Holder. That URL is the current target of Apple's own “submit a request” link. The authenticated form's exact labels and attestations must be read there; this document does not invent them from a third-party walkthrough.

## Fields observed in the authenticated conversion form

The coordinating session inspected the live form on 26 October 2026 and reported these fields. Only the known legal name, corrected D-U-N-S and factual additional-information text had been entered at that point; that was not a submission.

| Actual field | Prepared answer or evidence still needed |
| --- | --- |
| Region | Use the applicable Romania option from the actual selector; company country is source-backed. |
| Organization Name | `LEANZERO S.R.L.` |
| D-U-N-S Number | `303218286` |
| Are you the founder/cofounder? Yes / No | Website bio and organization metadata support founder status. The Account Holder answers the actual declaration; no legal-authority checkbox was accepted by this document's author. |
| Does the organization use a DBA, fictitious or trade name? Yes / No | Not established by reviewed sources. The website's LeanZero styling does not prove the legal answer. Owner confirmation or company records are needed. |
| Does the organization hold an Apple Developer Program membership? Active / Expired / None | The visible provider is the individual's current membership. It does not prove whether the company has a separate current or expired membership; use account records or owner confirmation. |
| Tax ID associated with the current individual membership: personal / organization / none | Inspect the current individual membership's tax configuration or obtain the owner's answer. Company CUI `51336260` does not establish this answer. |
| Additional information | The independently reviewed text below. |

## Prepared company details

These are source-backed values to map to the fields Apple actually displays. Preserve the legal spelling returned by D&B/registration documents if it differs in capitalization or punctuation.

| Field | Prepared value | Evidence and qualification |
| --- | --- | --- |
| Existing Account Holder | Mihai Perdum | Current Apple provider receipt. |
| Existing Developer Team ID | `ZZ8MTZ6NRZ` | Current provider receipt, existing signing configuration. |
| Target legal entity | LEANZERO S.R.L. | Exact legal spelling in the owner-supplied D&B resolution email. The public website styles it LeanZero SRL, a Romanian limited liability company. |
| D-U-N-S | `303218286` | Owner's corrected number, supplied on 26 October 2026 with the D&B resolution email dated 7 September 2026. Not independently checked in Apple's live lookup yet. |
| Country | Romania | Live company legal notice. |
| Street/address line | STR TOAMNEI, NR.23G, Camera1 | Exact street/address from the supplied D&B resolution; the published registered office uses Str. Toamnei 23 G, CAM. 1. |
| City/locality | Bragadiru | Published registered office. |
| County/state | Ilfov | Published registered office. |
| Postal code | `077025` | Published registered office; preserve leading zero. |
| Website | `https://leanzero.net` | Live company site. |
| Company contact email | `office@leanzero.net` | Live company legal notice; organizational domain. |
| Published business phone | `+40 764 498 476` | Website source at `src/app/(pages)/support-sla/page.tsx:64` and `src/app/(pages)/contact/page.tsx:144`. Use as business contact if the authenticated form calls for that contact. |
| Romanian trade register number | `J2025012835002` | Live company legal notice. This is not a D-U-N-S number. |
| CUI | `51336260` | Live company legal notice. This is not a D-U-N-S number. |
| EUID | `ROONRC.J2025012835002` | Live company legal notice. |

The [live privacy policy](https://leanzero.net/privacy-policy), last updated 21 October 2026, and [terms](https://leanzero.net/terms-of-service) identify the same company, registered office, trade register number, CUI and EUID. Repository source agrees at `src/app/(pages)/privacy-policy/page.tsx:27-37` and `src/app/(pages)/terms-of-service/page.tsx:23-32` in `/Users/mihaiperdum/Projects/LeanZero-website`.

The corrected D&B evidence is the owner's supplied resolution email: case `10901169`, tracking `10841239`, resolved 7 September 2026. It states that the company record was created/verified against the national registry, with D-U-N-S `303218286`, legal name `LEANZERO S.R.L.`, address `STR TOAMNEI, NR.23G, Camera1`, `BRAGADIRU 077025`, Romania. This replaces the earlier supplied candidate. The email was supplied by the owner; this lane has not independently retrieved the D&B or Apple record.

The website's team source says Mihai started LeanZero, and its migration-page organization metadata names him as founder (`src/app/(pages)/team/page.tsx:146-150`, `src/app/(pages)/services/atlassian-migrations/components/MigrationsContent.tsx:311-313`). Those records support the preparation; they do not replace the Account Holder's own founder/legal-authority declaration to Apple.

## D-U-N-S verification

Use [Apple's organization lookup](https://developer.apple.com/enroll/duns-lookup/) to confirm that the corrected D-U-N-S `303218286` resolves to `LEANZERO S.R.L.` with the expected location. The owner has supplied the D&B creation/verification resolution; the remaining check is Apple's live recognition of that record, not a search for another number or a new D-U-N-S request.

Apple explains that D-U-N-S identifies a business on a location-specific basis and must be registered to the legal entity. The lookup asks for legal entity name, headquarters address, mailing address and work contact information. The published address above is explicitly the registered office: do not silently relabel it as a different headquarters or mailing address. Confirm the record/owner's actual addresses when mapping those fields. If D&B's record needs correction, follow the correction route rather than substituting the Romanian CUI or inventing a second company. [Apple D-U-N-S requirements and lookup procedure](https://developer.apple.com/help/account/membership/D-U-N-S/).

## Draft additional information

The text below is the exact additional-information draft reviewed for the live form. It makes no founder/legal-authority attestation and claims no attachment has been supplied. Its D&B statement rests on the resolution email supplied by the owner.

I'd like to convert my existing individual Apple Developer membership, team ZZ8MTZ6NRZ, to LEANZERO S.R.L. before publishing our first iPhone and iPad app. D-U-N-S: 303218286. D&B confirmed this number on 7 September 2026 after verification through the national registry. Registered address: STR TOAMNEI, NR.23G, Camera 1, BRAGADIRU 077025, Romania. Website: https://leanzero.net. Business contact: office@leanzero.net. Please let me know which company documents you need for the conversion.

Independent review on 26 October 2026: PASS for this exact additional-information text against the supplied D&B resolution, current Apple provider receipt and public legal notice. The reviewer also confirmed that founder metadata does not establish legal binding authority, and that DBA status, another company membership and the current individual's Tax ID classification remain unproved by these sources.

## Remaining owner inputs and verification

There is no need to ask for the company name, published registered address, existing team or D-U-N-S number again. The remaining items are:

1. Confirm Apple's lookup recognizes `303218286` as `LEANZERO S.R.L.` at the location in the supplied D&B resolution. This may be completed from the authenticated lookup without another owner question if the record is unambiguous.
2. The Account Holder completes the actual founder/cofounder declaration. Do not submit an attestation merely because website metadata says “founder”. If Apple asks for legal binding authority in this or a later step, the owner must answer that separately.
3. Resolve the actual form's DBA/trade-name, other company membership, and current individual membership Tax ID answers. The reviewed sources do not establish them; account evidence or the owner's direct answer can. Do not turn the company CUI into an invented answer about the individual's tax setup.
4. If a later lookup or form asks for headquarters/mailing information that differs from the published registered office, or a personal business contact distinct from the published company contact, obtain those precise differences. Do not ask again where the live form/account already resolves them.
5. Supply current company registration or authority documents only if Apple requests them. The exact document list, translation/certification requirements and any additional verification are determined by Apple; none is invented or marked attached here.

Apple's organization-enrollment guidance also calls for legal entity status, authority to bind it, a domain-associated work email and a working organization website. LeanZero's public records supply the company/contact/website context; Apple verifies eligibility. [Apple organization requirements](https://developer.apple.com/help/account/membership/program-enrollment).

Do not promise a conversion completion date or that team identifiers/certificates will remain unchanged: record Apple's actual response and instructions. After Apple confirms conversion, re-read membership/provider details and the seller display in App Store Connect, resolve the agreements/status Apple actually presents, then recheck the final archive's team/profile against the resulting account. Keep the first public release on hold until the owner-selected organization seller identity is confirmed. A support request acknowledgment alone is not conversion completion.
