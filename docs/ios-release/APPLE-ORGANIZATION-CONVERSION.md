# Convert the existing Apple membership to LEANZERO S.R.L.

Submitted 26 October 2026 through Apple's authenticated individual-to-organization form, after the owner confirmed the form facts. Apple acknowledged receipt; organization conversion remains pending. The owner accepted the updated Developer Program License Agreement; the refreshed portal recorded issue14October and acceptance26October (see release README). No App Store build was uploaded.

## Existing account

Keep the existing Apple Developer Program membership and team `ZZ8MTZ6NRZ`. The pre-submission App Store Connect provider response identifies `developerProgram=INDIVIDUAL`, `entityType=INDIVIDUAL`, seller `Mihai Perdum`, provider `6213b7a9-0a22-4f2b-a0da-524be5e2a657`, with the user's `LEGAL`, `CIPS` and `ADMIN` roles. The source is the read-only provider receipt `/tmp/goose-appstore-providers.json`, captured during this release preparation. The same team already serves Goose's Mac signing/notarization and the iOS archive/export work.

The provider response also contains `isSignupComplete=false`. Do not infer from that field alone which screen or agreement remains: inspect the authenticated Apple account. A signed/exported iOS archive establishes signing capability, not the desired organization seller identity or publication readiness.

Apple's official conversion procedure is an account-update request by the Account Holder. It specifically requires the requester to be the organization's founder/cofounder and to provide its D-U-N-S number; Apple may request business documents. This is the existing-membership conversion route, not a new enrollment or app transfer. Changing the personal Apple Account display name does not change an individual membership's App Store seller name. [Apple account-update requirements](https://developer.apple.com/help/account/membership/updating-your-account-information/).

The request was submitted through [Apple's individual-to-organization form](https://developer.apple.com/contact/request/migrate-individual-account), signed in as the existing Account Holder. That URL is the target of Apple's own “submit a request” link. The coordinating session read the actual form; its fields below do not come from a third-party walkthrough.

## Fields observed in the authenticated conversion form

The coordinating session inspected and submitted the live form on 26 October 2026. The owner expressly confirmed co-founder status, no separate registered alias, no separate company membership, and use of the company's CUI when enrolling in the current individual membership. The final answers were recorded as follows.

| Actual field | Submitted answer and basis |
| --- | --- |
| Region | Present in the authenticated form. Company country is Romania; the exact selected region label was not copied into the submission receipt. |
| Organization Name | `LEANZERO S.R.L.` |
| D-U-N-S Number | `303218286` |
| Are you the founder/cofounder? Yes / No | Yes. The owner explicitly confirmed he is a co-founder. |
| Does the organization use a DBA, fictitious or trade name? Yes / No | No. The owner explicitly confirmed no separate registered alias; the business uses LeanZero, matching the company name. |
| Does the organization hold an Apple Developer Program membership? Active / Expired / None | No separate company membership / None. The owner confirmed this is their only Apple Developer membership. |
| Tax ID associated with the current individual membership: personal / organization / none | Organization Tax ID. The owner confirmed he used company CUI `51336260` when enrolling. |
| Additional information | The exact submitted text below. |

## Company details used for preparation

These are the source-backed company details used for the request. The exact D&B legal spelling was preserved in the Organization Name field.

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

The website's team source says Mihai started LeanZero, and its migration-page organization metadata names him as founder (`src/app/(pages)/team/page.tsx:146-150`, `src/app/(pages)/services/atlassian-migrations/components/MigrationsContent.tsx:311-313`). These were corroborating context; the submitted co-founder answer rests on the owner's explicit confirmation, not on website metadata alone.

## D-U-N-S verification

The request used D-U-N-S `303218286` for `LEANZERO S.R.L.`, based on the owner's supplied D&B creation/verification resolution. This record contains no independently captured result from [Apple's organization lookup](https://developer.apple.com/enroll/duns-lookup/). Apple's review of the submitted identity remains pending; a new D-U-N-S request is not indicated.

Apple explains that D-U-N-S identifies a business on a location-specific basis and must be registered to the legal entity. The lookup asks for legal entity name, headquarters address, mailing address and work contact information. The published address above is explicitly the registered office: do not silently relabel it as a different headquarters or mailing address. Confirm the record/owner's actual addresses when mapping those fields. If D&B's record needs correction, follow the correction route rather than substituting the Romanian CUI or inventing a second company. [Apple D-U-N-S requirements and lookup procedure](https://developer.apple.com/help/account/membership/D-U-N-S/).

## Exact submitted additional information

The text below is the final additional-information text submitted in the live form. The co-founder, membership and enrollment-tax statements were confirmed by the owner before submission. The D&B statement rests on the resolution email supplied by the owner. No attachment was claimed.

I'd like to convert my existing individual Apple Developer membership, team ZZ8MTZ6NRZ, to LEANZERO S.R.L. before publishing our first iPhone and iPad app. I'm a co-founder. This is our only Apple Developer membership; the company does not hold a separate membership. D-U-N-S: 303218286. D&B confirmed this number on 7 September 2026 after verification through the national registry. Registered address: STR TOAMNEI, NR.23G, Camera 1, BRAGADIRU 077025, Romania. Company tax ID (CUI): 51336260, which I used when enrolling. We use the LeanZero name, matching the company name. Website: https://leanzero.net. Business contact: office@leanzero.net. Please let me know which company documents you need for the conversion.

Independent review on 26 October 2026: the updated text passed against the supplied D&B resolution, current Apple provider receipt, public legal notice and explicit owner confirmations. The reviewer initially identified that the separate No DBA answer was not established merely by finding no alternative name; the owner then explicitly confirmed no separate registered alias before submission. Completed owner answers must not be reopened as missing facts.

## Submission receipt

The authenticated response at `https://developer.apple.com/contact/request/migrate-individual-account` displayed:

“Thanks for contacting us.”

“We’ll review your request and get back to you within one (1) business day.”

The screenshot `/tmp/goose-apple-conversion-submitted.png`, recorded at `2026-10-26T19:49:01Z`, was independently inspected and shows those exact words and the Individual to Organization Membership Update breadcrumb. No case number was shown. The one-business-day statement concerns Apple's reply/review, not a promised completion date for conversion. The screenshot is a transient local evidence path; the response text is preserved here.

## Pending after submission

1. Await Apple's response and any requested verification documents. Supply only the documents Apple actually asks for; no document list or attachment is invented here.
2. Program License Agreement acceptance was verified26October; do not ask the owner again absent a new agreement.
3. After Apple confirms conversion, re-read membership/provider details and the seller display in App Store Connect, resolve any actual remaining agreements/status, then recheck the final archive's team/profile against the resulting account. No App Store upload has occurred.

Apple's organization-enrollment guidance also calls for legal entity status, authority to bind it, a domain-associated work email and a working organization website. LeanZero's public records supply the company/contact/website context; Apple verifies eligibility. [Apple organization requirements](https://developer.apple.com/help/account/membership/program-enrollment).

Do not promise a conversion completion date or that team identifiers/certificates will remain unchanged: record Apple's actual response and instructions. Keep the first public release on hold until the owner-selected organization seller identity is confirmed. This acknowledgment proves request submission, not conversion completion.

## Agreement verified after request

The authenticated Apple Developer account, freshly reloaded on 26 October 2026, records Apple Developer Program License Agreement issued 14 October 2026, accepted 26 October 2026. The owner accepted it in the browser. The previous agreement warning is gone. This does not confirm organization conversion.

## Read-only recheck, 29 October 2026

Existing credential authentication via Apple altool list-providers succeeded during the morning recheck. The same provider still reports developerProgram/entityType INDIVIDUAL, name Mihai Perdum, developerTeamId ZZ8MTZ6NRZ. Organization conversion is not yet reflected in this source. No new conversion request, app record or upload was made. The ambiguous isSignupComplete=false field still does not establish a specific missing action.
