# Microsoft Graph read-only provider contract

**Research scope:** Microsoft 365/Outlook calendar access for Kimi Nola's Windows-first, opt-in, read-only integration.

**Source policy:** This report uses only Microsoft first-party documentation on `learn.microsoft.com`. Claims are cited inline. No production code was changed.

## Executive recommendation

Use a **delegated public-client authorization-code flow with PKCE through the user's system browser**. Do not ship a client secret or certificate in the Windows application: Microsoft classifies desktop applications as public clients that cannot safely hold application secrets, and public clients are for user-delegated access.[[MSAL public clients](#msal-public-and-confidential-clients)][[Auth-code flow](#oauth-20-authorization-code-flow)]

For the callback, prefer the documented desktop **loopback** configuration (`http://localhost` for system-browser apps) with a per-run state value, PKCE verifier/challenge, and a local listener. Microsoft also documents an Electron custom-scheme example (`msal{client-id}://auth`), but the reviewed first-party docs do not document Tauri's Windows custom-scheme registration. Treat the custom-scheme route as an integration risk and keep loopback as the MVP path.[[Desktop registration](#desktop-app-registration-and-msal-configuration)][[Redirect URI guidance](#redirect-uri-rules)]

Request only these delegated scopes for the initial calendar contract:

- `Calendars.ReadBasic` — Microsoft marks it as the least-privileged permission for listing calendars, listing a calendar view, listing events, and listing recurring-event instances.[[List calendars](#calendar-discovery)][[Calendar view](#bounded-event-listing-and-time-zones)][[List events](#event-listing-and-recurrence)][[List instances](#recurring-series-instances)]
- `offline_access` — needed if the client is to receive refresh tokens for background/open-app refresh. Microsoft documents that refresh tokens are returned only when this scope was requested.[[Refresh tokens](#refresh-tokens-consent-and-revocation)][[Auth-code flow](#oauth-20-authorization-code-flow)]

Add `User.Read` only if the product chooses to call `GET /me` for a Graph profile. It is the least-privileged permission for that operation; it is not required merely to call `/me/calendars` with delegated calendar access.[[Get user](#account-discovery)]

Discover calendars with `GET /me/calendars`, then for each user-selected calendar query `calendarView` over an eight-date local window: **today's local midnight through the local midnight after today + seven days**. This represents “current local day plus the next seven days.” Send explicit ISO 8601 offsets in `startDateTime`/`endDateTime`, and use `Prefer: outlook.timezone="<Windows time-zone name>"` so returned event times are rendered in the Windows device's local zone. `calendarView` is the correct endpoint because it expands occurrences, exceptions, and single instances; `/events` returns single instances and series masters instead.[[Calendar view](#bounded-event-listing-and-time-zones)][[List events](#event-listing-and-recurrence)][[DateTimeTimeZone](#time-zone-representation)]

Use `calendarView/delta` per calendar and per fixed date range for incremental refresh between bounded full refreshes. Save opaque `@odata.deltaLink` values only after consuming all `@odata.nextLink` pages. Because the delta state encodes the range, perform a full refresh when the local eight-date window rolls forward, and fall back to a full refresh if a cursor is invalid or unusable. Do not make ETags the sync cursor; `@odata.etag` and `changeKey` are useful local version hints, while delta links are the documented change-tracking mechanism.[[Delta query](#delta-query-and-version-hints)][[Paging](#pagination)][[Event resource](#event-resource-and-version-fields)]

## 1. OAuth and public-client contract

### Public-client classification

Microsoft's MSAL documentation distinguishes public and confidential clients. Desktop and mobile applications are public clients; their distributed source/compiled code can be inspected, so they cannot safely keep a client secret. Public clients can obtain delegated tokens on behalf of a signed-in user but cannot prove their own application identity with a secret.[[MSAL public clients](#msal-public-and-confidential-clients)]

Contract implications:

1. Register Kimi Nola as a **public client** and ship only the application/client ID and authority configuration.
2. Never embed a client secret, certificate private key, or other confidential credential in the open-source desktop binary.[[Auth-code flow](#oauth-20-authorization-code-flow)][[MSAL public clients](#msal-public-and-confidential-clients)]
3. Use a Microsoft-supported authentication library where the chosen Rust/Tauri integration permits it; Microsoft's protocol page explicitly recommends a supported authentication library instead of hand-crafting raw OAuth requests.[[Auth-code flow](#oauth-20-authorization-code-flow)]

### System-browser authorization-code flow with PKCE

Microsoft documents the following shape for desktop/native apps:

1. Construct a public-client application and enumerate cached accounts.
2. Try silent token acquisition for the selected MSAL account.
3. If silent acquisition requires UI, open an interactive authorization request in the system browser.
4. Receive the authorization response at the registered redirect URI and redeem the short-lived authorization code with the matching PKCE verifier.
5. Store the resulting token cache/refresh token in the Windows credential store, not in the repository, plaintext configuration, or ordinary app data.

The silent-first pattern is shown in the desktop token-acquisition guidance; the interactive guidance also shows cached-account lookup followed by interactive acquisition, and the Node/Python examples use authorization code + PKCE for public clients.[[Desktop acquire token](#desktop-token-acquisition)][[Interactive desktop acquisition](#interactive-desktop-acquisition)][[Desktop registration](#desktop-app-registration-and-msal-configuration)]

The authorization request must:

- use `response_type=code`;
- include the required delegated scopes;
- include a fresh, unpredictable `state` value and verify the returned value before redemption; and
- use PKCE, preferably `code_challenge_method=S256`, then send the matching `code_verifier` to the token endpoint.[[Auth-code flow](#oauth-20-authorization-code-flow)]

The authorization code is short-lived (the protocol documentation says typically about one minute), so the callback handler should redeem it once, reject duplicate/replayed callbacks, and report an expired/mismatched code as an authentication failure rather than retrying the same code.[[Auth-code flow](#oauth-20-authorization-code-flow)]

### Loopback versus custom redirect

**Recommended MVP: loopback.** Microsoft's desktop-app registration instructions specify `http://localhost` for apps using system browsers. The redirect URI used in the request must match a registered URI exactly, subject to Microsoft's documented localhost matching rules.[[Desktop registration](#desktop-app-registration-and-msal-configuration)][[Auth-code flow](#oauth-20-authorization-code-flow)]

The redirect-URI guidance adds important native-app constraints:

- `http` is accepted only for localhost/loopback URIs; non-localhost HTTP is invalid.
- For localhost redirects, the port is ignored when matching, which supports an ephemeral local port. The path must still identify the intended callback.
- Microsoft recommends the IP-literal loopback address `127.0.0.1` over `localhost` for firewall/interface reliability. An HTTP `127.0.0.1` URI currently has to be added through the application manifest rather than the portal's redirect-URI text box.
- IPv6 loopback `[::1]` is not currently supported.
- Redirect URIs are case-sensitive and must match the registered path; do not rely on a case-normalized callback.

These rules make a loopback listener a good fit for a Windows desktop app, but the exact production registration (`http://localhost` versus a manifest-added `http://127.0.0.1/<path>`) must be tested against the chosen MSAL/Rust implementation.[[Redirect URI guidance](#redirect-uri-rules)]

**Custom scheme: supported in a narrower documented example.** Microsoft's desktop configuration page lists `msal{Your_Application/Client_Id}://auth` for Node.js Electron apps. It does not provide a Tauri-specific Windows registration or scheme-handling recipe in the reviewed pages. A custom scheme therefore remains a fallback only after validating Windows protocol registration, exact Entra matching, collision resistance, and callback ownership in the packaged Tauri build.[[Desktop registration](#desktop-app-registration-and-msal-configuration)]

## 2. Permissions, consent, refresh, and disconnect behavior

### Least privilege

Microsoft's current Graph endpoint pages mark `Calendars.ReadBasic` as the least-privileged delegated permission for:

- `GET /me/calendars`;
- `GET .../calendarView`; and
- `GET .../events/{id}/instances`.

The event-listing page also includes `Calendars.ReadBasic` in the delegated permission table. `Calendars.Read`, `Calendars.ReadWrite`, and `Calendars.Read.Shared` are higher-privileged alternatives on those pages.[[List calendars](#calendar-discovery)][[Calendar view](#bounded-event-listing-and-time-zones)][[List events](#event-listing-and-recurrence)][[List instances](#recurring-series-instances)]

Therefore the initial provider contract should not ask for `Calendars.Read`, `Calendars.ReadWrite`, `Calendars.Read.Shared`, group-calendar permissions, mail permissions, attendee/contacts permissions, or application permissions. If shared/delegated calendars become a requirement, treat `Calendars.Read.Shared` as an explicit follow-up permission decision and test the exact mailbox-sharing cases; do not silently broaden the initial consent request.

The supplied permissions-reference URL timed out during this research pass. The least-privilege conclusion above is independently supported by the endpoint-specific Microsoft Graph permission tables cited above; the timed-out page was not repeatedly retried.

### Consent behavior

Delegated permissions are scopes applied on behalf of a signed-in user. User consent can be requested statically through the app registration or dynamically/incrementally at sign-in; incremental consent applies to delegated permissions. Microsoft recommends listing admin-privileged permissions statically when they are needed, but this contract deliberately avoids such permissions.[[Permissions and consent](#permissions-and-consent)][[Consent request patterns](#consent-request-patterns)]

For a read-only desktop connection:

- Request the minimum calendar scope at the moment the user chooses **Connect calendar**.
- Use `prompt=consent` only for an explicit “re-consent/update permissions” action, not on every refresh. Microsoft's interactive guidance documents `prompt=consent` for forcing the consent dialog and `prompt=select_account` for account selection.[[Interactive desktop acquisition](#interactive-desktop-acquisition)][[Auth-code flow](#oauth-20-authorization-code-flow)]
- If a tenant policy or external-tenant rule requires admin consent, show an actionable “administrator approval required” state rather than treating it as a calendar-empty result. Microsoft notes that users in external tenants may not be able to consent themselves and an administrator must grant consent.[[Consent request patterns](#consent-request-patterns)]
- Adding or removing permissions in an app registration does not itself grant or revoke previously granted access; consent must be granted or revoked separately. This is relevant when an open-source release changes its registered scope set.[[Consent request patterns](#consent-request-patterns)]

### Refresh tokens

Microsoft documents that:

- `offline_access` is required for refresh tokens to be returned.
- Refresh tokens are used to obtain new access/refresh-token pairs after an access token expires.
- Refresh tokens for non-SPA scenarios have a documented default lifetime of 90 days, but they may expire or be revoked earlier.
- A new refresh token may be returned on refresh; replace the stored token with the new one and securely delete the old one.
- Refresh tokens are bound to the user and client, not to one resource or tenant, but the resulting access token still requires consent for the requested resource/scopes.
- Refresh-token use can fail because of expiration, revocation, insufficient privilege, or a required interactive step; the application must handle token-endpoint errors and send the user through interactive sign-in again when required.[[Refresh tokens](#refresh-tokens-consent-and-revocation)][[Auth-code flow](#oauth-20-authorization-code-flow)]

The cache contract should therefore be:

- Store the MSAL token cache/refresh token in the Windows credential store.
- On refresh, atomically replace the old token with the new token if one is returned.
- On `invalid_grant`, `interaction_required`, `consent_required`, or equivalent MSAL “UI required” results, mark the provider connection as requiring sign-in; do not loop unattended.
- Keep the access token out of the calendar database and out of event snapshots.

### Revocation and disconnect

Microsoft says refresh tokens can be revoked by the sign-in service because of user/admin actions or credential changes. The refresh-token guidance specifically covers user revocation, administrator revocation, administrator password resets, and other revocation cases; the consent overview notes that a consent prompt can reappear after previously granted consent is revoked.[[Refresh tokens](#refresh-tokens-consent-and-revocation)][[Permissions and consent](#permissions-and-consent)]

Provider behavior should distinguish:

- **Local disconnect:** delete the local MSAL account/token cache and all provider-owned calendar cache data. This prevents Kimi Nola from using the local credential again, but should not be represented as proof that remote Entra consent was revoked.
- **Remote revocation/expiry:** when refresh or Graph calls fail because the grant is no longer valid, delete the unusable local credential, retain only a non-secret disconnected status, and require interactive sign-in.
- **User cancellation:** map `access_denied` to a user-cancelled connection attempt without destructive cache erasure unless the user explicitly chose disconnect.

The reviewed Microsoft pages describe sign-in-service, user, and administrator revocation, but do not define a desktop “disconnect” UX or a user-triggered revocation endpoint. Remote consent revocation should therefore remain an explicit user/admin action in the product documentation, not an implicit promise made by Kimi Nola's local disconnect button.

## 3. Account and calendar discovery

### Account discovery

MSAL's desktop guidance shows enumerating cached accounts and trying `AcquireTokenSilent` for the chosen account before interactive acquisition. The interactive guidance recommends account selection when multiple identities are present; the authorization endpoint also supports `prompt=select_account`.[[Desktop token acquisition](#desktop-token-acquisition)][[Interactive desktop acquisition](#interactive-desktop-acquisition)][[Auth-code flow](#oauth-20-authorization-code-flow)]

Recommended local account model:

- Treat each MSAL account as a separate provider account; never assume only one Microsoft identity exists on the machine.
- Keep a stable MSAL account identifier plus tenant/account display metadata returned by the auth library. Do not parse Microsoft Graph access tokens for authorization decisions; Microsoft warns that tokens for Microsoft services can use formats the app should not depend on.[[Auth-code flow](#oauth-20-authorization-code-flow)]
- Reuse the selected account for silent refresh. If the account disappears from the MSAL cache or silent acquisition requires UI, surface “sign-in required.”

If the app wants an authoritative Graph profile for display or account confirmation, use `GET /me` with delegated `User.Read`. Microsoft identifies `User.Read` as the least-privileged permission for `/me` and explicitly says `/me` requires a signed-in user/delegated permission; application permissions are not supported for `/me`.[[Get user](#account-discovery)]

For a strict calendar-only consent surface, avoid the extra `/me` request and use the MSAL account identity supplied by the authentication library. Add `User.Read` only if the UX needs Graph profile fields.

### Calendar discovery

Use:

```http
GET https://graph.microsoft.com/v1.0/me/calendars
Authorization: Bearer <access-token>
```

The current Graph endpoint is `user-list-calendars`; it returns the user's calendars and supports calendar-group variants. Microsoft marks `Calendars.ReadBasic` as least privileged for both work/school and personal delegated accounts. The response includes a calendar `id` and display/configuration fields such as `name`, `color`, `changeKey`, `canViewPrivateItems`, `canEdit`, and `owner`.[[List calendars](#calendar-discovery)]

Persist only the minimum calendar metadata needed for Settings and refresh routing: provider account key, calendar ID, selected/deselected state, display name/color, and the last successful discovery time. Follow calendar-list `@odata.nextLink` if present; Graph's paging contract requires consuming every page rather than assuming the first response is complete.[[List calendars](#calendar-discovery)][[Pagination](#pagination)]

The old supplied URL `https://learn.microsoft.com/en-us/graph/api/calendar-list?view=graph-rest-1.0` returned a 404 during this pass. Microsoft currently exposes the same operation at `https://learn.microsoft.com/en-us/graph/api/user-list-calendars?view=graph-rest-1.0`; the 404 URL was not repeatedly retried.

## 4. Bounded event listing contract

### Local window

Kimi Nola's settled product window is the current local day plus the next seven days. Make the boundary explicit:

- `windowStart`: today's midnight in the Windows device time zone.
- `windowEnd`: midnight at the start of the day after `windowStart + 7 calendar days`.
- Query the half-open interval `[windowStart, windowEnd)` as the product's local-cache convention. The provider's required parameters are the corresponding ISO 8601 `startDateTime` and `endDateTime` values with explicit UTC offsets.

The half-open interpretation is a Kimi Nola boundary rule; Microsoft documents the required time range and offset interpretation but does not define the product's inclusive/exclusive cache convention. Test midnight-boundary events explicitly.

### Request shape

For each selected calendar, use the calendar-view route, for example:

```http
GET https://graph.microsoft.com/v1.0/me/calendars/{calendar-id}/calendarView?startDateTime=2026-08-23T00:00:00-05:00&endDateTime=2026-08-31T00:00:00-05:00
Authorization: Bearer <access-token>
Prefer: outlook.timezone="Central Standard Time"
```

The dates above are illustrative only; the implementation must compute the current Windows-local date and the correct offset at each boundary, including daylight-saving transitions. Microsoft says `startDateTime` and `endDateTime` are required, are ISO 8601 values, and are interpreted using the offset supplied in the value. If no offset is supplied, Graph interprets the value as UTC. The `Prefer: outlook.timezone` header controls the time zone used for returned start/end values; without it, returned times are UTC.[[Calendar view](#bounded-event-listing-and-time-zones)]

The endpoint supports OData query parameters. Use a minimal `$select` projection consistent with `Calendars.ReadBasic`, for example:

```text
id,iCalUId,subject,start,end,isAllDay,type,seriesMasterId,recurrence,isCancelled,changeKey
```

Do not persist body, attendees, locations, join URLs, or other meeting-provider content; those fields are outside Kimi Nola's settled calendar contract. `calendarView` has one important projection limitation: `createdDateTime` and `lastModifiedDateTime` do not support `$select`; if those properties are required later, test an unprojected request and keep them out of the initial minimal snapshot.[[Calendar view](#bounded-event-listing-and-time-zones)][[Event resource](#event-resource-and-version-fields)]

If event IDs must remain stable when an item moves between containers, test adding `Prefer: IdType="ImmutableId"`; the event resource documentation says the default ID can change when an item is moved and points to that header for immutable identifiers.[[Event resource](#event-resource-and-version-fields)]

### Recurrence expansion

Do not use `/me/calendars/{id}/events` as the primary upcoming-event endpoint: Microsoft says that list contains single-instance meetings and series masters. Use `calendarView`, which returns occurrences, exceptions, and single instances in the requested range.[[List events](#event-listing-and-recurrence)][[Calendar view](#bounded-event-listing-and-time-zones)]

For a specific series-master ID, the `/instances` operation returns occurrences and exceptions in a requested time range. Event `type` can distinguish `occurrence`, `exception`, and `seriesMaster`, and `seriesMasterId` links an instance back to its recurring series. This is useful as a targeted fallback or verification call, but a range-wide `calendarView` is the simpler primary contract.[[List instances](#recurring-series-instances)][[Event resource](#event-resource-and-version-fields)]

The local event record should retain the provider event ID, `iCalUId` when returned, `seriesMasterId` when returned, `type`, start/end, cancellation flag, and `changeKey`/ETag hints. A recurring occurrence's `iCalUId` is different for each occurrence, so do not use it alone as the series key.[[Event resource](#event-resource-and-version-fields)]

### All-day events

Microsoft defines `isAllDay=true` as an event that lasts all day. For all-day events, start and end must be midnight and must use the same time zone, whether the event lasts one day or multiple days.[[Event resource](#event-resource-and-version-fields)]

Provider normalization:

- Preserve the all-day flag and provider start/end dates.
- Interpret all-day events as local-date spans in the Windows device time zone, not as timed instants to be shifted across UTC.
- Keep them display/link-only per the settled Kimi Nola product decision; they must not enrich a new Meeting prompt.
- Test single-day, multi-day, recurring, and DST-adjacent all-day events.

### Time-zone representation

Graph's `dateTimeTimeZone` resource carries a `dateTime` string plus a `timeZone` string. Microsoft lists Windows time zones as generally supported and also documents an additional set of time-zone identifiers. Use the Windows device zone for the product window and request/response preference, while preserving the provider's returned zone metadata for diagnostics.[[DateTimeTimeZone](#time-zone-representation)]

Do not construct the range by taking UTC midnight and then labeling it local. Compute local calendar midnights first, then serialize each boundary with its actual offset. This is especially important on DST transition dates. The offset-handling rule is directly supported by the calendar-view documentation; the local-midnight algorithm is the Kimi Nola implementation contract.[[Calendar view](#bounded-event-listing-and-time-zones)]

## 5. Incremental refresh, ETags, pagination, and throttling

### Delta query and version hints

Microsoft's event delta documentation supports a full synchronization followed by incremental synchronization for a fixed calendar view:

```http
GET /me/calendarView/delta?startDateTime={start}&endDateTime={end}
```

For a selected calendar, use the corresponding calendar-specific calendar-view delta route. Track each calendar independently. A response returns either `@odata.nextLink` while pages remain or `@odata.deltaLink` when the round is complete. The state token is opaque and encodes the original time range and query parameters; the client must copy the complete URL rather than reconstructing it. `@odata.deltaLink` can be saved for the next round. Delta responses can contain updated/new events and deletion markers such as `@removed`.[[Delta query](#delta-query-and-version-hints)]

Delta-specific rules:

- The initial request includes the fixed `startDateTime` and `endDateTime` range.
- `$select` is not supported on the delta query; do not assume delta responses have the same minimal projection as a normal `calendarView` request. Persist only the allowed local fields.
- `Prefer: odata.maxpagesize={n}` can bound the number of events returned per delta response.
- Store the delta link only after the entire page chain has succeeded and its changes have been applied atomically.
- Discard the cursor and perform a full sync when the local eight-date window changes, the selected calendar changes, or Graph returns a cursor/transport error that the client cannot recover from. The reviewed delta page does not promise a specific cursor-expiry status, so the exact error-to-full-sync mapping remains an integration-test requirement.[[Delta query](#delta-query-and-version-hints)]

Event responses expose `@odata.etag` in Microsoft's examples, and the event resource defines `changeKey` as a version identifier that changes whenever the event changes. Store either/both as local no-op/deduplication hints, but use delta links—not ETags—as the authoritative incremental-sync cursor. The reviewed pages do not prescribe an `If-None-Match` calendar polling strategy.[[Delta query](#delta-query-and-version-hints)][[Event resource](#event-resource-and-version-fields)]

### Pagination

Microsoft Graph may server-page collections. When `@odata.nextLink` is present, issue a GET against the **entire URL** until it is absent; do not extract and rebuild only `$skip`/`$skipToken`. A page may contain zero or more results and page sizes vary by API.[[Pagination](#pagination)]

For `calendarView`, `$top` has a documented minimum of 1 and maximum of 1000. Use a bounded page size if needed, but always honor `@odata.nextLink`. For delta, use `Prefer: odata.maxpagesize` and follow the returned next-link chain.[[Calendar view](#bounded-event-listing-and-time-zones)][[Delta query](#delta-query-and-version-hints)]

### Throttling

Microsoft Graph returns **HTTP 429 Too Many Requests** when a throttling threshold is exceeded and includes a suggested delay in `Retry-After` on the failed response. The documented recovery loop is: wait the specified number of seconds, retry, and continue using the latest recommended delay if another 429 occurs. Do not immediately retry; if no `Retry-After` is supplied, use exponential backoff. Microsoft also recommends change tracking rather than frequent polling to reduce throttling risk.[[Throttling](#throttling)]

For Kimi Nola:

- Apply bounded, jittered backoff around the provider request layer, respecting `Retry-After` exactly when present.
- Do not discard a successful page cursor because a later page was throttled; resume the same complete next-link after the delay.
- Prefer delta refreshes and one request per selected calendar over repeated full scans.
- Surface a stale-but-usable cache state if the app exhausts its retry budget; never silently replace a failed refresh with an empty calendar.

### Error states

The Microsoft identity-platform documentation lists these relevant authorization/token errors:

- `access_denied`: the user denied consent; stop the current connect attempt and explain that consent is required.
- `interaction_required` / `login_required`: silent acquisition cannot complete; reopen interactive browser sign-in.
- `invalid_grant`: the authorization code or PKCE verifier is invalid/expired, or a refresh grant is no longer valid; discard the unusable transaction/credential and start a fresh interactive flow.
- `consent_required`: send the user back through authorization with valid scopes.
- `invalid_scope`: fix the registered/requested scope set; do not retry unchanged.
- `unauthorized_client` / `invalid_request`: treat as app-registration or protocol configuration defects and show a diagnostic path.
- `temporarily_unavailable` / `server_error`: retry after a delay with a bounded policy.

The auth-code page also documents `trace_id`, `correlation_id`, timestamp, and provider error description fields; retain those in diagnostic logs without logging access or refresh tokens.[[Auth-code flow](#oauth-20-authorization-code-flow)]

For Graph requests, require a successful 2xx response and a valid collection before mutating the local cache. The calendar event-list page documents a special private-item failure: when accessing private items in another mailbox, the caller needs mailbox `FullAccess` or delegate `CanViewPrivateItems`; otherwise Graph can return “The specified object was not found in the store.” This is primarily a shared/delegated-mailbox risk and reinforces keeping the initial contract to the signed-in user's own calendars.[[List events](#event-listing-and-recurrence)]

The supplied `https://learn.microsoft.com/en-us/graph/errors` URL returned a 404 during this pass. It was not repeatedly retried; the error mapping above relies on the first-party OAuth error tables, endpoint-specific behavior, and throttling guidance that were available.

## 6. Entra app registration, tenant, and testing constraints

### Registration shape

Microsoft's desktop configuration instructions require:

1. Create an app registration and record the Application (client) ID and Directory (tenant) ID.
2. Under **Authentication**, add the **Mobile and desktop applications** platform.
3. For system-browser apps, add the documented `http://localhost` redirect URI; for embedded-browser apps, use Microsoft's native-client URI instead. Kimi Nola should use the system-browser path.
4. Under **Advanced settings**, set **Allow public client flows** to **Yes**.

Microsoft's desktop scenario sample uses a single-tenant registration, while the broader app-registration guidance describes single-tenant, multitenant-organizational, organizational-plus-personal, and personal-only audiences. Choose the audience to match the release contract rather than silently accepting Microsoft's sample's single-tenant default.[[Desktop registration](#desktop-app-registration-and-msal-configuration)][[Register an application](#register-an-application)][[Supported account types](#supported-account-types)]

For Kimi Nola's intended “Microsoft 365/Outlook” audience:

- **Work/school only:** use an organizational audience and an `organizations` or tenant-specific authority.
- **Work/school plus Outlook.com/personal Microsoft accounts:** register for the audience that includes organizational and personal accounts and use the corresponding `common`/audience authority. Microsoft documents `common`, `organizations`, `consumers`, and tenant identifiers as valid authorization-endpoint tenant values.[[Supported account types](#supported-account-types)][[Auth-code flow](#oauth-20-authorization-code-flow)]
- **Single organization deployment:** a tenant-specific authority is simpler and avoids cross-tenant consent, but it is not a general open-source consumer distribution contract.

Application registrations are tenant-owned and Microsoft says an application object cannot be moved between tenants. Keep development and production registrations separate, with only the redirects each environment needs.[[Register an application](#register-an-application)][[Redirect URI guidance](#redirect-uri-rules)]

### Redirect URI rules that affect release/testing

Microsoft's redirect guidance requires exact registered matches and documents these constraints:

- Redirect URIs must normally use HTTPS; HTTP is an exception for localhost/loopback.
- Paths are case-sensitive.
- A pathless redirect may be returned with a trailing slash; register/use the exact path form expected by the library.
- Query parameters are not allowed for app registrations that support personal Microsoft accounts; avoid query-bearing custom redirects.
- Wildcards are unsupported for apps serving personal accounts and multitenant organizational accounts; use an absolute redirect.
- A single registration has finite redirect-URI limits (256 for organizational-only audiences; 100 when personal accounts are included), and each URI has a 256-character limit.
- Localhost ports are ignored for matching, but the path and redirect type still matter; do not register multiple localhost URIs that differ only by port.
- `127.0.0.1` HTTP redirects may require manifest editing; `[::1]` is unsupported.

These constraints should be covered by packaged-build and fresh-install tests, not only a local dev run.[[Redirect URI guidance](#redirect-uri-rules)]

### Tenant/admin-consent test matrix

At minimum, test:

1. A work/school account in the app owner's tenant.
2. A work/school account in a different tenant when the release is multitenant.
3. A personal Microsoft account if that audience is enabled.
4. A user who cancels consent, a user whose grant was revoked, and a user whose refresh token has expired/revoked.
5. Multiple cached MSAL accounts and account switching.
6. MFA/conditional-access and a browser already signed in to another Microsoft account.
7. A calendar with recurring timed events, exceptions, cancellations, all-day events, and a DST boundary.
8. Multiple selected calendars, an empty calendar, paginated results, a 429 response, and a failed delta cursor.

For external tenants, Microsoft says customer users may be unable to consent themselves and an administrator must grant consent. The test plan must therefore include an administrator-consent path and an explicit “admin approval required” UX.[[Consent request patterns](#consent-request-patterns)][[Register an application](#register-an-application)]

The application-registration quickstart also notes that the registering account must have at least the Application Developer role (or equivalent administrative capability). This is a release-process prerequisite, not a permission to request from end users.[[Register an application](#register-an-application)]

## 7. Proposed Kimi Nola provider contract

### Inputs

- MSAL public-client configuration: client ID, selected authority/audience, registered redirect URI.
- A selected MSAL account identifier.
- A user-maintained set of selected calendar IDs.
- Windows device time zone and current local date.

### Authentication

- Delegated public-client auth-code + PKCE using the system browser.
- Scopes: `Calendars.ReadBasic offline_access`; add `User.Read` only when `/me` profile discovery is deliberately enabled.
- Silent acquisition first; interactive acquisition only on UI-required/authentication errors.
- Fresh PKCE verifier/challenge and `state` for every interactive transaction.
- No client secret/certificate in the desktop app.
- Credential storage in the Windows credential store; no tokens in event rows or logs.

### Discovery

- Enumerate/select MSAL accounts locally.
- `GET /me/calendars` for calendar discovery and selection.
- Follow all calendar-list pages.
- Store provider account key + calendar ID + minimal display metadata.

### Read operation

For each selected calendar:

- Compute `[local midnight today, local midnight after today + 7 days)`.
- Call that calendar's `calendarView` with explicit offsets and `Prefer: outlook.timezone`.
- Follow all `@odata.nextLink` pages.
- Keep only the minimal event fields needed for upcoming-event display/linking and recurrence identity.
- Treat all-day events as local-date display/link-only items.
- Do not write to Graph; omit all create/update/delete/response operations from this seam.

### Refresh operation

- Full range sync on first connection, selection change, local-date window rollover, or unrecoverable delta failure.
- Otherwise use per-calendar `calendarView/delta` with its opaque delta link.
- Apply upserts and `@removed` deletions transactionally; publish a new cache generation only after all selected calendars complete.
- Respect `Retry-After` and preserve stale-cache state on exhausted retries.

### Local data boundary

The provider seam may return only: provider/account identity, calendar identity/name/color, event identity, subject/title, timed/all-day start/end, recurrence linkage/type, cancellation, and sync/version metadata. Attendees, bodies/descriptions, locations, join URLs, attachments, and provider write operations stay outside this contract.

## 8. Unresolved risks and blockers

1. **Permissions-reference fetch:** the supplied permissions-reference URL timed out. Endpoint-specific Graph docs independently confirm `Calendars.ReadBasic` as least privileged, but the parent should recheck the permissions-reference page when available.
2. **Supplied calendar-list URL:** `api/calendar-list` returned 404; use the current `user-list-calendars` URL.
3. **Supplied Graph errors URL:** `graph/errors` returned 404; use the OAuth error tables, endpoint docs, and throttling page until Microsoft exposes the current replacement.
4. **Tauri custom scheme:** Microsoft's reviewed custom-scheme example is for Electron, not Tauri. Loopback is the lower-risk MVP choice; custom URI registration needs a packaged Windows end-to-end test.
5. **Loopback registration variant:** Microsoft documents `http://localhost` for system-browser desktop apps and separately recommends `127.0.0.1` for reliability, with manifest editing for HTTP. Validate the exact redirect URI and ephemeral-port behavior with the selected Rust/MSAL stack.
6. **`Calendars.ReadBasic` projection:** the endpoint pages establish least privilege, but the exact fields available under this permission and the behavior of `$select` versus delta responses must be verified against work/school and personal test accounts.
7. **Moving-window delta semantics:** Microsoft documents fixed-range calendar-view delta, but the reviewed page does not state a specific cursor-expiry status. Implement full-sync fallback for any unusable cursor and verify the behavior in integration tests.
8. **Shared/delegated calendars:** the initial contract intentionally avoids `Calendars.Read.Shared`; if user-selected shared calendars are required, permissions, mailbox visibility, private-item behavior, and consent must be re-researched and tested before broadening scope.
9. **Authority/audience decision:** the parent product spec still needs to choose work/school only versus work/school plus personal Microsoft accounts. That choice controls supported-account-type registration, redirect restrictions, authority, and admin-consent testing.

## Citation anchors used above

The inline citations above jump to these first-party source links:

- <a id="msal-public-and-confidential-clients"></a>[MSAL public and confidential client applications](https://learn.microsoft.com/en-us/entra/identity-platform/msal-client-applications)
- <a id="oauth-20-authorization-code-flow"></a>[OAuth 2.0 authorization code flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)
- <a id="desktop-app-registration-and-msal-configuration"></a>[Desktop app registration and MSAL configuration](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration)
- <a id="redirect-uri-rules"></a>[Redirect URI rules](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url)
- <a id="refresh-tokens-consent-and-revocation"></a>[Refresh tokens, consent, and revocation](https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens)
- <a id="desktop-token-acquisition"></a>[Desktop token acquisition](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-acquire-token)
- <a id="interactive-desktop-acquisition"></a>[Interactive desktop acquisition](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-acquire-token-interactive)
- <a id="calendar-discovery"></a>[Calendar discovery](https://learn.microsoft.com/en-us/graph/api/user-list-calendars?view=graph-rest-1.0)
- <a id="bounded-event-listing-and-time-zones"></a>[Bounded event listing and time zones](https://learn.microsoft.com/en-us/graph/api/calendar-list-calendarview?view=graph-rest-1.0)
- <a id="event-listing-and-recurrence"></a>[Event listing and recurrence](https://learn.microsoft.com/en-us/graph/api/calendar-list-events?view=graph-rest-1.0)
- <a id="recurring-series-instances"></a>[Recurring-series instances](https://learn.microsoft.com/en-us/graph/api/event-list-instances?view=graph-rest-1.0)
- <a id="permissions-and-consent"></a>[Permissions and consent](https://learn.microsoft.com/en-us/entra/identity-platform/permissions-consent-overview)
- <a id="consent-request-patterns"></a>[Consent request patterns](https://learn.microsoft.com/en-us/entra/identity-platform/consent-types-developer)
- <a id="account-discovery"></a>[Account discovery](https://learn.microsoft.com/en-us/graph/api/user-get?view=graph-rest-1.0)
- <a id="time-zone-representation"></a>[Time-zone representation](https://learn.microsoft.com/en-us/graph/api/resources/datetimetimezone?view=graph-rest-1.0)
- <a id="delta-query-and-version-hints"></a>[Delta query and version hints](https://learn.microsoft.com/en-us/graph/delta-query-events?view=graph-rest-1.0)
- <a id="event-resource-and-version-fields"></a>[Event resource and version fields](https://learn.microsoft.com/en-us/graph/api/resources/event?view=graph-rest-1.0)
- <a id="pagination"></a>[Pagination](https://learn.microsoft.com/en-us/graph/paging)
- <a id="throttling"></a>[Throttling](https://learn.microsoft.com/en-us/graph/throttling)
- <a id="register-an-application"></a>[Register an application](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)
- <a id="supported-account-types"></a>[Supported account types](https://learn.microsoft.com/en-us/entra/identity-platform/v2-supported-account-types)

## Sources

All sources below are Microsoft first-party documentation links.

### Identity and app registration

- [Desktop app that calls web APIs: Acquire a token](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-acquire-token) — silent-first acquisition pattern and desktop flows.
- [Desktop app that calls web APIs: Acquire a token interactively](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-acquire-token-interactive) — interactive system browser, account cache, PKCE, prompts, and custom UI.
- [Configure desktop apps that call web APIs](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration) — public-client registration, system-browser `http://localhost`, Electron custom scheme, and public-client-flow setting.
- [Microsoft identity platform and OAuth 2.0 authorization code flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow) — PKCE, state, tenant values, exact redirect matching, code redemption, refresh, and OAuth error codes.
- [Public and confidential client applications (MSAL)](https://learn.microsoft.com/en-us/entra/identity-platform/msal-client-applications) — why distributed desktop apps are public clients and cannot use client secrets.
- [Redirect URI (reply URL) outline and restrictions](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url) — localhost/loopback, exact matching, ports, schemes, wildcard/query restrictions, and URI limits.
- [Refresh tokens in the Microsoft identity platform](https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens) — lifetime, replacement, expiration, and revocation behavior.
- [Overview of permissions and consent](https://learn.microsoft.com/en-us/entra/identity-platform/permissions-consent-overview) — delegated versus application permissions and consent behavior.
- [Developer's guide to requesting permissions and consent](https://learn.microsoft.com/en-us/entra/identity-platform/consent-types-developer) — static/incremental consent, admin consent, external tenants, and adding/removing permissions.
- [Register an application in Microsoft Entra ID](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app) — app registration, account audiences, tenant ownership, and registration prerequisites.
- [Supported account types](https://learn.microsoft.com/en-us/entra/identity-platform/v2-supported-account-types) — organizational, multitenant, personal-account, and flow support.

### Microsoft Graph calendar and transport

- [Get a user](https://learn.microsoft.com/en-us/graph/api/user-get?view=graph-rest-1.0) — `/me`, delegated-only behavior, and `User.Read` least privilege.
- [List calendars](https://learn.microsoft.com/en-us/graph/api/user-list-calendars?view=graph-rest-1.0) — current calendar-discovery endpoint and `Calendars.ReadBasic` least privilege.
- [List calendarView](https://learn.microsoft.com/en-us/graph/api/calendar-list-calendarview?view=graph-rest-1.0) — bounded range, offsets, response time zone, recurrence expansion, paging, and permission table.
- [List events](https://learn.microsoft.com/en-us/graph/api/calendar-list-events?view=graph-rest-1.0) — series-master behavior, permissions, and private-mailbox caveat.
- [List instances](https://learn.microsoft.com/en-us/graph/api/event-list-instances?view=graph-rest-1.0) — occurrences/exceptions for a series master and permission table.
- [Get incremental changes to events in a calendar view](https://learn.microsoft.com/en-us/graph/delta-query-events?view=graph-rest-1.0) — per-calendar fixed-range delta, opaque next/delta links, deletion markers, and page sizing.
- [event resource type](https://learn.microsoft.com/en-us/graph/api/resources/event?view=graph-rest-1.0) — event types, recurrence linkage, all-day constraints, IDs, `changeKey`, and ETag examples.
- [dateTimeTimeZone resource type](https://learn.microsoft.com/en-us/graph/api/resources/datetimetimezone?view=graph-rest-1.0) — date/time plus time-zone representation.
- [Paging Microsoft Graph data](https://learn.microsoft.com/en-us/graph/paging) — complete `@odata.nextLink` handling.
- [Microsoft Graph throttling guidance](https://learn.microsoft.com/en-us/graph/throttling) — 429, `Retry-After`, backoff, and change-tracking guidance.
