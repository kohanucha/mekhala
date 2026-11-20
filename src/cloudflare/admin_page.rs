//! The single-page admin console, served as one HTML document.
//!
//! Keeping it here means there is no build step and no second deployable: the
//! page ships with the Worker that answers its API. It holds no secret and no
//! server data; every byte it shows is fetched from `/admin/api/*` after the
//! browser has been authorized, which is why the document itself can be served
//! without a credential.
//!
//! [`PAGE`] carries [`NONCE_PLACEHOLDER`] in place of a CSP nonce. The handler
//! replaces it with a fresh value per response, so the inline script and style
//! run only in the document that shipped them.

/// Marker replaced with a per-response CSP nonce.
pub const NONCE_PLACEHOLDER: &str = "__CSP_NONCE__";

/// The console: list registrations, add one, remove one, re-seal all.
pub const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Mekhala admin</title>
<style nonce="__CSP_NONCE__">
  :root { color-scheme: light dark; }
  body { font: 15px/1.5 system-ui, sans-serif; margin: 0; padding: 2rem 1rem; }
  main { max-width: 60rem; margin: 0 auto; }
  h1 { font-size: 1.4rem; margin: 0 0 1rem; }
  table { width: 100%; border-collapse: collapse; margin-top: 1.5rem; }
  th, td { text-align: left; padding: .4rem .5rem; border-bottom: 1px solid #8884; }
  th { font-weight: 600; font-size: .85rem; text-transform: uppercase; letter-spacing: .04em; }
  form { display: grid; gap: .6rem; max-width: 34rem; margin-top: 1rem; }
  label { display: grid; gap: .2rem; font-size: .85rem; }
  input { padding: .45rem; font: inherit; }
  button { padding: .45rem .8rem; font: inherit; cursor: pointer; width: fit-content; }
  .muted { opacity: .75; }
  .row-actions { text-align: right; }
  code { font-size: .85em; }
</style>
</head>
<body>
<main>
  <h1>Lightning Addresses</h1>
  <p id="status" class="muted">Loading…</p>

  <section id="secret-panel" hidden>
    <p class="muted">This relay asks for the admin secret.</p>
    <form id="secret-form">
      <label>Admin secret
        <input id="secret" type="password" autocomplete="off" required>
      </label>
      <button type="submit">Use secret</button>
    </form>
  </section>

  <form id="register-form">
    <label>Username
      <input id="username" autocomplete="off" required maxlength="64">
    </label>
    <label>Wallet connection string
      <input id="nwc" type="password" autocomplete="off" required>
    </label>
    <button type="submit">Register</button>
  </form>

  <table>
    <thead>
      <tr><th>Username</th><th>Wallet</th><th>Granted methods</th><th></th></tr>
    </thead>
    <tbody id="rows"></tbody>
  </table>

  <p><button id="more" type="button" hidden>Load more</button></p>
  <p><button id="reseal" type="button">Re-seal all records</button></p>
</main>
<script nonce="__CSP_NONCE__">
(function () {
  'use strict';

  var STORAGE_KEY = 'mekhala-admin-secret';
  var statusEl = document.getElementById('status');
  var rowsEl = document.getElementById('rows');
  var moreButton = document.getElementById('more');
  var nextCursor = null;
  var shown = 0;
  var secretPanel = document.getElementById('secret-panel');
  var secretInput = document.getElementById('secret');
  var usernameInput = document.getElementById('username');
  var nwcInput = document.getElementById('nwc');
  var secret = sessionStorage.getItem(STORAGE_KEY) || '';

  function say(text) {
    statusEl.textContent = text;
  }

  function requestHeaders() {
    var headers = { 'Content-Type': 'application/json' };
    if (secret) {
      headers.Authorization = 'Bearer ' + secret;
    }
    return headers;
  }

  function api(method, path, body) {
    var init = { method: method, headers: requestHeaders(), credentials: 'same-origin' };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
    }
    return fetch(path, init).then(function (response) {
      return response.json().catch(function () { return null; }).then(function (data) {
        if (response.status === 401) {
          secretPanel.hidden = false;
          throw new Error('Not authorized. Enter the admin secret.');
        }
        if (!response.ok) {
          throw new Error((data && data.error) || ('Request failed: ' + response.status));
        }
        return data;
      });
    });
  }

  function cell(text) {
    var td = document.createElement('td');
    td.textContent = text;
    return td;
  }

  function appendRows(registrations) {
    registrations.forEach(function (registration) {
      var row = document.createElement('tr');
      row.appendChild(cell(registration.username));
      row.appendChild(cell(registration.wallet_pubkey || 'unknown'));
      row.appendChild(cell((registration.methods || []).join(', ') || 'not verified'));

      var actions = document.createElement('td');
      actions.className = 'row-actions';
      var remove = document.createElement('button');
      remove.type = 'button';
      remove.textContent = 'Remove';
      remove.addEventListener('click', function () {
        if (!window.confirm('Remove ' + registration.username + '?')) {
          return;
        }
        api('DELETE', '/admin/api/registrations/' + encodeURIComponent(registration.username))
          .then(function () {
            say('Removed ' + registration.username + '.');
            return refresh();
          })
          .catch(function (error) { say(error.message); });
      });
      actions.appendChild(remove);
      row.appendChild(actions);
      rowsEl.appendChild(row);
    });
  }

  // The API returns one page at a time, so the cursor decides what "Load more"
  // asks for and the total is summed here rather than reported by the relay.
  function showPage(data) {
    var registrations = data.registrations || [];
    appendRows(registrations);
    shown += registrations.length;
    nextCursor = data.next_cursor || null;
    moreButton.hidden = !nextCursor;

    if (shown === 0) {
      say('No registrations yet.');
    } else if (nextCursor) {
      say('Showing ' + shown + ' registrations; more remain.');
    } else {
      say(shown + ' registration(s).');
    }
  }

  function listingPath(cursor) {
    return '/admin/api/registrations' +
      (cursor ? '?cursor=' + encodeURIComponent(cursor) : '');
  }

  function refresh() {
    rowsEl.textContent = '';
    shown = 0;
    nextCursor = null;
    moreButton.hidden = true;
    return api('GET', listingPath(null)).then(showPage).catch(function (error) {
      say(error.message);
    });
  }

  document.getElementById('secret-form').addEventListener('submit', function (event) {
    event.preventDefault();
    secret = secretInput.value.trim();
    if (!secret) {
      return;
    }
    sessionStorage.setItem(STORAGE_KEY, secret);
    secretPanel.hidden = true;
    refresh();
  });

  document.getElementById('register-form').addEventListener('submit', function (event) {
    event.preventDefault();
    var username = usernameInput.value.trim();
    var nwcUri = nwcInput.value.trim();
    say('Registering ' + username + '…');
    api('POST', '/admin/api/registrations', { username: username, nwc_uri: nwcUri })
      .then(function () {
        nwcInput.value = '';
        return refresh();
      })
      .then(function () { say('Registered ' + username + '.'); })
      .catch(function (error) { say(error.message); });
  });

  moreButton.addEventListener('click', function () {
    if (!nextCursor) {
      return;
    }
    var cursor = nextCursor;
    api('GET', listingPath(cursor)).then(showPage).catch(function (error) {
      say(error.message);
    });
  });

  // One page per request keeps each Worker invocation inside its KV budget,
  // so a rotation walks the namespace by following the cursor to the end.
  document.getElementById('reseal').addEventListener('click', function () {
    var totals = { scanned: 0, resealed: 0, failed: 0 };

    function step(cursor) {
      var path = '/admin/api/reseal' +
        (cursor ? '?cursor=' + encodeURIComponent(cursor) : '');
      return api('POST', path).then(function (summary) {
        totals.scanned += summary.scanned || 0;
        totals.resealed += summary.resealed || 0;
        totals.failed += summary.failed || 0;
        if (summary.next_cursor) {
          say('Re-sealing… ' + totals.scanned + ' scanned.');
          return step(summary.next_cursor);
        }
        say('Scanned ' + totals.scanned + ', re-sealed ' + totals.resealed +
          ', failed ' + totals.failed + '.');
      });
    }

    say('Re-sealing…');
    step(null).catch(function (error) { say(error.message); });
  });

  refresh();
})();
</script>
</body>
</html>
"#;
