var __defProp = Object.defineProperty;
var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

// .wrangler/tmp/bundle-Odd3O6/checked-fetch.js
var urls = /* @__PURE__ */ new Set();
function checkURL(request, init) {
  const url = request instanceof URL ? request : new URL(
    (typeof request === "string" ? new Request(request, init) : request).url
  );
  if (url.port && url.port !== "443" && url.protocol === "https:") {
    if (!urls.has(url.toString())) {
      urls.add(url.toString());
      console.warn(
        `WARNING: known issue with \`fetch()\` requests to custom HTTPS ports in published Workers:
 - ${url.toString()} - the custom port will be ignored when the Worker is published using the \`wrangler deploy\` command.
`
      );
    }
  }
}
__name(checkURL, "checkURL");
globalThis.fetch = new Proxy(globalThis.fetch, {
  apply(target, thisArg, argArray) {
    const [request, init] = argArray;
    checkURL(request, init);
    return Reflect.apply(target, thisArg, argArray);
  }
});

// build/index.js
import { WorkerEntrypoint as lt } from "cloudflare:workers";
import $ from "./cdec81231c587581bfb84d3095a18fd35dba8f13-index_bg.wasm";
var N = globalThis.__worker_init_state = { criticalError: false, instanceId: 0 };
var I = class {
  static {
    __name(this, "I");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, _t.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_containerstartupoptions_free(t, 0);
  }
  get enableInternet() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_containerstartupoptions_enableInternet(this.__wbg_ptr), t === 16777215 ? void 0 : t !== 0;
  }
  get entrypoint() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    o(), t = _.__wbg_get_containerstartupoptions_entrypoint(this.__wbg_ptr);
    var e = ut(t[0], t[1]).slice();
    return _.__wbindgen_free(t[0], t[1] * 4, 4), e;
  }
  get env() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_containerstartupoptions_env(this.__wbg_ptr), t;
  }
  set enableInternet(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_containerstartupoptions_enableInternet(this.__wbg_ptr, b(t) ? 16777215 : t ? 1 : 0);
  }
  set entrypoint(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e = ft(t, _.__wbindgen_malloc), r = g;
    o(), _.__wbg_set_containerstartupoptions_entrypoint(this.__wbg_ptr, e, r);
  }
  set env(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_containerstartupoptions_env(this.__wbg_ptr, t);
  }
};
Symbol.dispose && (I.prototype[Symbol.dispose] = I.prototype.free);
var x = class {
  static {
    __name(this, "x");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, it.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_intounderlyingbytesource_free(t, 0);
  }
  get autoAllocateChunkSize() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.intounderlyingbytesource_autoAllocateChunkSize(this.__wbg_ptr), t >>> 0;
  }
  cancel() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t = this.__destroy_into_raw();
    o(), _.intounderlyingbytesource_cancel(t);
  }
  pull(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    return o(), e = _.intounderlyingbytesource_pull(this.__wbg_ptr, t), e;
  }
  start(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.intounderlyingbytesource_start(this.__wbg_ptr, t);
  }
  get type() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.intounderlyingbytesource_type(this.__wbg_ptr), rt[t];
  }
};
Symbol.dispose && (x.prototype[Symbol.dispose] = x.prototype.free);
var E = class {
  static {
    __name(this, "E");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, ot.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_intounderlyingsink_free(t, 0);
  }
  abort(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e = this.__destroy_into_raw(), r;
    return o(), r = _.intounderlyingsink_abort(e, t), r;
  }
  close() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t = this.__destroy_into_raw(), e;
    return o(), e = _.intounderlyingsink_close(t), e;
  }
  write(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    return o(), e = _.intounderlyingsink_write(this.__wbg_ptr, t), e;
  }
};
Symbol.dispose && (E.prototype[Symbol.dispose] = E.prototype.free);
var R = class {
  static {
    __name(this, "R");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, st.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_intounderlyingsource_free(t, 0);
  }
  cancel() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t = this.__destroy_into_raw();
    o(), _.intounderlyingsource_cancel(t);
  }
  pull(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    return o(), e = _.intounderlyingsource_pull(this.__wbg_ptr, t), e;
  }
};
Symbol.dispose && (R.prototype[Symbol.dispose] = R.prototype.free);
var S = class {
  static {
    __name(this, "S");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, ct.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_minifyconfig_free(t, 0);
  }
  get css() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_minifyconfig_css(this.__wbg_ptr), t !== 0;
  }
  get html() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_minifyconfig_html(this.__wbg_ptr), t !== 0;
  }
  get js() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_minifyconfig_js(this.__wbg_ptr), t !== 0;
  }
  set css(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_minifyconfig_css(this.__wbg_ptr, t);
  }
  set html(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_minifyconfig_html(this.__wbg_ptr, t);
  }
  set js(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_minifyconfig_js(this.__wbg_ptr, t);
  }
};
Symbol.dispose && (S.prototype[Symbol.dispose] = S.prototype.free);
var k = class {
  static {
    __name(this, "k");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, V.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_nwcrelay_free(t, 0);
  }
  alarm() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.nwcrelay_alarm(this.__wbg_ptr), t;
  }
  fetch(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let e;
    return o(), e = _.nwcrelay_fetch(this.__wbg_ptr, t), e;
  }
  constructor(t, e) {
    let r;
    return o(), r = _.nwcrelay_new(t, e), this.__wbg_ptr = r >>> 0, Object.defineProperty(this, "__wbg_inst", { value: s, writable: true }), V.register(this, { ptr: r >>> 0, instance: s }, this), this;
  }
  webSocketClose(t, e, r, i) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let c = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), a = g, f;
    return o(), f = _.nwcrelay_webSocketClose(this.__wbg_ptr, t, e, c, a, i), f;
  }
  webSocketError(t, e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let r;
    return o(), r = _.nwcrelay_webSocketError(this.__wbg_ptr, t, e), r;
  }
  webSocketMessage(t, e) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let r;
    return o(), r = _.nwcrelay_webSocketMessage(this.__wbg_ptr, t, e), r;
  }
};
Symbol.dispose && (k.prototype[Symbol.dispose] = k.prototype.free);
var F = class {
  static {
    __name(this, "F");
  }
  __destroy_into_raw() {
    let t = this.__wbg_ptr;
    return this.__wbg_ptr = 0, at.unregister(this), t;
  }
  free() {
    let t = this.__destroy_into_raw();
    o(), _.__wbg_r2range_free(t, 0);
  }
  get length() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_r2range_length(this.__wbg_ptr), t[0] === 0 ? void 0 : t[1];
  }
  get offset() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_r2range_offset(this.__wbg_ptr), t[0] === 0 ? void 0 : t[1];
  }
  get suffix() {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    let t;
    return o(), t = _.__wbg_get_r2range_suffix(this.__wbg_ptr), t[0] === 0 ? void 0 : t[1];
  }
  set length(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_r2range_length(this.__wbg_ptr, !b(t), b(t) ? 0 : t);
  }
  set offset(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_r2range_offset(this.__wbg_ptr, !b(t), b(t) ? 0 : t);
  }
  set suffix(t) {
    if (this.__wbg_inst !== void 0 && this.__wbg_inst !== s) throw new Error("Invalid stale object from previous Wasm instance");
    o(), _.__wbg_set_r2range_suffix(this.__wbg_ptr, !b(t), b(t) ? 0 : t);
  }
};
Symbol.dispose && (F.prototype[Symbol.dispose] = F.prototype.free);
function U() {
  s++, v = null, j = null, typeof numBytesDecoded < "u" && (numBytesDecoded = 0), typeof g < "u" && (g = 0), Q = false, _ = new WebAssembly.Instance($, K()).exports, _.__wbindgen_start();
}
__name(U, "U");
function H() {
  let n;
  return o(), n = _.__worker_init_state(), n;
}
__name(H, "H");
function J(n, t, e) {
  let r;
  return o(), r = _.fetch(n, t, e), r;
}
__name(J, "J");
function G() {
  o(), _.init();
}
__name(G, "G");
function K() {
  return { __proto__: null, "./index_bg.js": { __proto__: null, __wbg_String_8564e559799eccda: /* @__PURE__ */ __name(function(t, e) {
    let r = String(e), i = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), c = g;
    w().setInt32(t + 4, c, true), w().setInt32(t + 0, i, true);
  }, "__wbg_String_8564e559799eccda"), __wbg___wbindgen_debug_string_ab4b34d23d6778bd: /* @__PURE__ */ __name(function(t, e) {
    let r = L(e), i = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), c = g;
    w().setInt32(t + 4, c, true), w().setInt32(t + 0, i, true);
  }, "__wbg___wbindgen_debug_string_ab4b34d23d6778bd"), __wbg___wbindgen_is_function_3baa9db1a987f47d: /* @__PURE__ */ __name(function(t) {
    return typeof t == "function";
  }, "__wbg___wbindgen_is_function_3baa9db1a987f47d"), __wbg___wbindgen_is_object_63322ec0cd6ea4ef: /* @__PURE__ */ __name(function(t) {
    let e = t;
    return typeof e == "object" && e !== null;
  }, "__wbg___wbindgen_is_object_63322ec0cd6ea4ef"), __wbg___wbindgen_is_string_6df3bf7ef1164ed3: /* @__PURE__ */ __name(function(t) {
    return typeof t == "string";
  }, "__wbg___wbindgen_is_string_6df3bf7ef1164ed3"), __wbg___wbindgen_is_undefined_29a43b4d42920abd: /* @__PURE__ */ __name(function(t) {
    return t === void 0;
  }, "__wbg___wbindgen_is_undefined_29a43b4d42920abd"), __wbg___wbindgen_string_get_7ed5322991caaec5: /* @__PURE__ */ __name(function(t, e) {
    let r = e, i = typeof r == "string" ? r : void 0;
    var c = b(i) ? 0 : h(i, _.__wbindgen_malloc, _.__wbindgen_realloc), a = g;
    w().setInt32(t + 4, a, true), w().setInt32(t + 0, c, true);
  }, "__wbg___wbindgen_string_get_7ed5322991caaec5"), __wbg___wbindgen_throw_6b64449b9b9ed33c: /* @__PURE__ */ __name(function(t, e) {
    throw new Error(l(t, e));
  }, "__wbg___wbindgen_throw_6b64449b9b9ed33c"), __wbg__wbg_cb_unref_b46c9b5a9f08ec37: /* @__PURE__ */ __name(function(t) {
    t._wbg_cb_unref();
  }, "__wbg__wbg_cb_unref_b46c9b5a9f08ec37"), __wbg_accept_356d0be4c95c6609: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      t.accept();
    }, arguments);
  }, "__wbg_accept_356d0be4c95c6609"), __wbg_addEventListener_8176dab41b09531c: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r, i) {
      t.addEventListener(l(e, r), i);
    }, arguments);
  }, "__wbg_addEventListener_8176dab41b09531c"), __wbg_body_0c3a51aec038a31a: /* @__PURE__ */ __name(function(t) {
    let e = t.body;
    return b(e) ? 0 : p(e);
  }, "__wbg_body_0c3a51aec038a31a"), __wbg_buffer_d0f5ea0926a691fd: /* @__PURE__ */ __name(function(t) {
    return t.buffer;
  }, "__wbg_buffer_d0f5ea0926a691fd"), __wbg_byobRequest_dc6aed9db01b12c6: /* @__PURE__ */ __name(function(t) {
    let e = t.byobRequest;
    return b(e) ? 0 : p(e);
  }, "__wbg_byobRequest_dc6aed9db01b12c6"), __wbg_byteLength_3e660e5661f3327e: /* @__PURE__ */ __name(function(t) {
    return t.byteLength;
  }, "__wbg_byteLength_3e660e5661f3327e"), __wbg_byteOffset_ecd62abe44dd28d4: /* @__PURE__ */ __name(function(t) {
    return t.byteOffset;
  }, "__wbg_byteOffset_ecd62abe44dd28d4"), __wbg_call_a24592a6f349a97e: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r) {
      return t.call(e, r);
    }, arguments);
  }, "__wbg_call_a24592a6f349a97e"), __wbg_cause_9e61ba47f40dd7e8: /* @__PURE__ */ __name(function(t) {
    return t.cause;
  }, "__wbg_cause_9e61ba47f40dd7e8"), __wbg_cf_1793f916b9902842: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      let e = t.cf;
      return b(e) ? 0 : p(e);
    }, arguments);
  }, "__wbg_cf_1793f916b9902842"), __wbg_cf_da3280bfacc59d75: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      let e = t.cf;
      return b(e) ? 0 : p(e);
    }, arguments);
  }, "__wbg_cf_da3280bfacc59d75"), __wbg_close_e6c8977a002e9e13: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      t.close();
    }, arguments);
  }, "__wbg_close_e6c8977a002e9e13"), __wbg_close_fb954dfaf67b5732: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      t.close();
    }, arguments);
  }, "__wbg_close_fb954dfaf67b5732"), __wbg_constructor_9f0cb60f616370a8: /* @__PURE__ */ __name(function(t) {
    return t.constructor;
  }, "__wbg_constructor_9f0cb60f616370a8"), __wbg_crypto_38df2bab126b63dc: /* @__PURE__ */ __name(function(t) {
    return t.crypto;
  }, "__wbg_crypto_38df2bab126b63dc"), __wbg_data_bb9dffdd1e99cf2d: /* @__PURE__ */ __name(function(t) {
    return t.data;
  }, "__wbg_data_bb9dffdd1e99cf2d"), __wbg_enqueue_4767ce322820c94d: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      t.enqueue(e);
    }, arguments);
  }, "__wbg_enqueue_4767ce322820c94d"), __wbg_error_2001591ad2463697: /* @__PURE__ */ __name(function(t) {
    console.error(t);
  }, "__wbg_error_2001591ad2463697"), __wbg_error_28b77b682ffaae05: /* @__PURE__ */ __name(function(t) {
    return t.error;
  }, "__wbg_error_28b77b682ffaae05"), __wbg_error_a6fa202b58aa1cd3: /* @__PURE__ */ __name(function(t, e) {
    let r, i;
    try {
      r = t, i = e, console.error(l(t, e));
    } finally {
      o(), _.__wbindgen_free(r, i, 1);
    }
  }, "__wbg_error_a6fa202b58aa1cd3"), __wbg_error_f536c7930d1c5c8d: /* @__PURE__ */ __name(function(t, e) {
    console.error(t, e);
  }, "__wbg_error_f536c7930d1c5c8d"), __wbg_fetch_c003c883aa7fa05e: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return t.fetch(e);
    }, arguments);
  }, "__wbg_fetch_c003c883aa7fa05e"), __wbg_getRandomValues_c44a50d8cfdaebeb: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      t.getRandomValues(e);
    }, arguments);
  }, "__wbg_getRandomValues_c44a50d8cfdaebeb"), __wbg_get_6011fa3a58f61074: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return Reflect.get(t, e);
    }, arguments);
  }, "__wbg_get_6011fa3a58f61074"), __wbg_get_7a3ccde226c74000: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return Reflect.get(t, e >>> 0);
    }, arguments);
  }, "__wbg_get_7a3ccde226c74000"), __wbg_get_aa7ea1c497b45090: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r, i) {
      let c = e.get(l(r, i));
      var a = b(c) ? 0 : h(c, _.__wbindgen_malloc, _.__wbindgen_realloc), f = g;
      w().setInt32(t + 4, f, true), w().setInt32(t + 0, a, true);
    }, arguments);
  }, "__wbg_get_aa7ea1c497b45090"), __wbg_get_dd110fba18ce2676: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return t.get(e);
    }, arguments);
  }, "__wbg_get_dd110fba18ce2676"), __wbg_headers_6022deb4e576fb8e: /* @__PURE__ */ __name(function(t) {
    return t.headers;
  }, "__wbg_headers_6022deb4e576fb8e"), __wbg_headers_cd7ea89df2f6ff86: /* @__PURE__ */ __name(function(t) {
    return t.headers;
  }, "__wbg_headers_cd7ea89df2f6ff86"), __wbg_idFromName_02f56c1895d6bcb6: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r) {
      return t.idFromName(l(e, r));
    }, arguments);
  }, "__wbg_idFromName_02f56c1895d6bcb6"), __wbg_instanceId_23752a922e5c7aef: /* @__PURE__ */ __name(function(t) {
    return t.instanceId;
  }, "__wbg_instanceId_23752a922e5c7aef"), __wbg_instanceof_Error_6872d63ba7922898: /* @__PURE__ */ __name(function(t) {
    let e;
    try {
      e = t instanceof Error;
    } catch {
      e = false;
    }
    return e;
  }, "__wbg_instanceof_Error_6872d63ba7922898"), __wbg_instanceof_Response_9b2d111407865ff2: /* @__PURE__ */ __name(function(t) {
    let e;
    try {
      e = t instanceof Response;
    } catch {
      e = false;
    }
    return e;
  }, "__wbg_instanceof_Response_9b2d111407865ff2"), __wbg_length_9f1775224cf1d815: /* @__PURE__ */ __name(function(t) {
    return t.length;
  }, "__wbg_length_9f1775224cf1d815"), __wbg_method_0384ffe0cd3d03b1: /* @__PURE__ */ __name(function(t, e) {
    let r = e.method, i = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), c = g;
    w().setInt32(t + 4, c, true), w().setInt32(t + 0, i, true);
  }, "__wbg_method_0384ffe0cd3d03b1"), __wbg_msCrypto_bd5a034af96bcba6: /* @__PURE__ */ __name(function(t) {
    return t.msCrypto;
  }, "__wbg_msCrypto_bd5a034af96bcba6"), __wbg_name_4049a9179544f842: /* @__PURE__ */ __name(function(t) {
    return t.name;
  }, "__wbg_name_4049a9179544f842"), __wbg_new_0c7403db6e782f19: /* @__PURE__ */ __name(function(t) {
    return new Uint8Array(t);
  }, "__wbg_new_0c7403db6e782f19"), __wbg_new_15a4889b4b90734d: /* @__PURE__ */ __name(function() {
    return u(function() {
      return new Headers();
    }, arguments);
  }, "__wbg_new_15a4889b4b90734d"), __wbg_new_227d7c05414eb861: /* @__PURE__ */ __name(function() {
    return new Error();
  }, "__wbg_new_227d7c05414eb861"), __wbg_new_5e360d2ff7b9e1c3: /* @__PURE__ */ __name(function(t, e) {
    return new Error(l(t, e));
  }, "__wbg_new_5e360d2ff7b9e1c3"), __wbg_new_aa8d0fa9762c29bd: /* @__PURE__ */ __name(function() {
    return new Object();
  }, "__wbg_new_aa8d0fa9762c29bd"), __wbg_new_ef3d3e5520df558b: /* @__PURE__ */ __name(function() {
    return u(function() {
      return new WebSocketPair();
    }, arguments);
  }, "__wbg_new_ef3d3e5520df558b"), __wbg_new_typed_323f37fd55ab048d: /* @__PURE__ */ __name(function(t, e) {
    try {
      var r = { a: t, b: e }, i = /* @__PURE__ */ __name((a, f) => {
        let d = r.a;
        r.a = 0;
        try {
          return nt(d, r.b, a, f);
        } finally {
          r.a = d;
        }
      }, "i");
      return new Promise(i);
    } finally {
      r.a = 0;
    }
  }, "__wbg_new_typed_323f37fd55ab048d"), __wbg_new_with_byte_offset_and_length_01848e8d6a3d49ad: /* @__PURE__ */ __name(function(t, e, r) {
    return new Uint8Array(t, e >>> 0, r >>> 0);
  }, "__wbg_new_with_byte_offset_and_length_01848e8d6a3d49ad"), __wbg_new_with_length_8c854e41ea4dae9b: /* @__PURE__ */ __name(function(t) {
    return new Uint8Array(t >>> 0);
  }, "__wbg_new_with_length_8c854e41ea4dae9b"), __wbg_new_with_opt_buffer_source_and_init_a16f51e86bb7c214: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return new Response(t, e);
    }, arguments);
  }, "__wbg_new_with_opt_buffer_source_and_init_a16f51e86bb7c214"), __wbg_new_with_opt_readable_stream_and_init_38c96167c370948a: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      return new Response(t, e);
    }, arguments);
  }, "__wbg_new_with_opt_readable_stream_and_init_38c96167c370948a"), __wbg_new_with_opt_str_and_init_c50a129670061a4d: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r) {
      return new Response(t === 0 ? void 0 : l(t, e), r);
    }, arguments);
  }, "__wbg_new_with_opt_str_and_init_c50a129670061a4d"), __wbg_node_84ea875411254db1: /* @__PURE__ */ __name(function(t) {
    return t.node;
  }, "__wbg_node_84ea875411254db1"), __wbg_process_44c7a14e11e9f69e: /* @__PURE__ */ __name(function(t) {
    return t.process;
  }, "__wbg_process_44c7a14e11e9f69e"), __wbg_prototypesetcall_a6b02eb00b0f4ce2: /* @__PURE__ */ __name(function(t, e, r) {
    Uint8Array.prototype.set.call(C(t, e), r);
  }, "__wbg_prototypesetcall_a6b02eb00b0f4ce2"), __wbg_queueMicrotask_5d15a957e6aa920e: /* @__PURE__ */ __name(function(t) {
    queueMicrotask(t);
  }, "__wbg_queueMicrotask_5d15a957e6aa920e"), __wbg_queueMicrotask_f8819e5ffc402f36: /* @__PURE__ */ __name(function(t) {
    return t.queueMicrotask;
  }, "__wbg_queueMicrotask_f8819e5ffc402f36"), __wbg_randomFillSync_6c25eac9869eb53c: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      t.randomFillSync(e);
    }, arguments);
  }, "__wbg_randomFillSync_6c25eac9869eb53c"), __wbg_removeEventListener_7bdf07404d9b24bd: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r, i) {
      t.removeEventListener(l(e, r), i);
    }, arguments);
  }, "__wbg_removeEventListener_7bdf07404d9b24bd"), __wbg_require_b4edbdcf3e2a1ef0: /* @__PURE__ */ __name(function() {
    return u(function() {
      return module.require;
    }, arguments);
  }, "__wbg_require_b4edbdcf3e2a1ef0"), __wbg_resolve_e6c466bc1052f16c: /* @__PURE__ */ __name(function(t) {
    return Promise.resolve(t);
  }, "__wbg_resolve_e6c466bc1052f16c"), __wbg_respond_008ca9525ae22847: /* @__PURE__ */ __name(function() {
    return u(function(t, e) {
      t.respond(e >>> 0);
    }, arguments);
  }, "__wbg_respond_008ca9525ae22847"), __wbg_send_15358dbe221c6258: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r) {
      t.send(l(e, r));
    }, arguments);
  }, "__wbg_send_15358dbe221c6258"), __wbg_set_022bee52d0b05b19: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r) {
      return Reflect.set(t, e, r);
    }, arguments);
  }, "__wbg_set_022bee52d0b05b19"), __wbg_set_1ffc463d4c541483: /* @__PURE__ */ __name(function() {
    return u(function(t, e, r, i, c) {
      t.set(l(e, r), l(i, c));
    }, arguments);
  }, "__wbg_set_1ffc463d4c541483"), __wbg_set_3d484eb794afec82: /* @__PURE__ */ __name(function(t, e, r) {
    t.set(C(e, r));
  }, "__wbg_set_3d484eb794afec82"), __wbg_set_criticalError_a317cc58ad3efd1a: /* @__PURE__ */ __name(function(t, e) {
    t.criticalError = e !== 0;
  }, "__wbg_set_criticalError_a317cc58ad3efd1a"), __wbg_set_headers_d567a640ab3a7735: /* @__PURE__ */ __name(function(t, e) {
    t.headers = e;
  }, "__wbg_set_headers_d567a640ab3a7735"), __wbg_set_instanceId_f98d02561c814f7f: /* @__PURE__ */ __name(function(t, e) {
    t.instanceId = e >>> 0;
  }, "__wbg_set_instanceId_f98d02561c814f7f"), __wbg_set_status_384b9a831b2c0723: /* @__PURE__ */ __name(function(t, e) {
    t.status = e;
  }, "__wbg_set_status_384b9a831b2c0723"), __wbg_stack_3b0d974bbf31e44f: /* @__PURE__ */ __name(function(t, e) {
    let r = e.stack, i = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), c = g;
    w().setInt32(t + 4, c, true), w().setInt32(t + 0, i, true);
  }, "__wbg_stack_3b0d974bbf31e44f"), __wbg_static_accessor_GLOBAL_8cfadc87a297ca02: /* @__PURE__ */ __name(function() {
    let t = typeof global > "u" ? null : global;
    return b(t) ? 0 : p(t);
  }, "__wbg_static_accessor_GLOBAL_8cfadc87a297ca02"), __wbg_static_accessor_GLOBAL_THIS_602256ae5c8f42cf: /* @__PURE__ */ __name(function() {
    let t = typeof globalThis > "u" ? null : globalThis;
    return b(t) ? 0 : p(t);
  }, "__wbg_static_accessor_GLOBAL_THIS_602256ae5c8f42cf"), __wbg_static_accessor_INIT_STATE_64fa719d0e4673b7: /* @__PURE__ */ __name(function() {
    return N;
  }, "__wbg_static_accessor_INIT_STATE_64fa719d0e4673b7"), __wbg_static_accessor_SELF_e445c1c7484aecc3: /* @__PURE__ */ __name(function() {
    let t = typeof self > "u" ? null : self;
    return b(t) ? 0 : p(t);
  }, "__wbg_static_accessor_SELF_e445c1c7484aecc3"), __wbg_static_accessor_WINDOW_f20e8576ef1e0f17: /* @__PURE__ */ __name(function() {
    let t = typeof window > "u" ? null : window;
    return b(t) ? 0 : p(t);
  }, "__wbg_static_accessor_WINDOW_f20e8576ef1e0f17"), __wbg_status_43e0d2f15b22d69f: /* @__PURE__ */ __name(function(t) {
    return t.status;
  }, "__wbg_status_43e0d2f15b22d69f"), __wbg_subarray_f8ca46a25b1f5e0d: /* @__PURE__ */ __name(function(t, e, r) {
    return t.subarray(e >>> 0, r >>> 0);
  }, "__wbg_subarray_f8ca46a25b1f5e0d"), __wbg_then_792e0c862b060889: /* @__PURE__ */ __name(function(t, e, r) {
    return t.then(e, r);
  }, "__wbg_then_792e0c862b060889"), __wbg_then_8e16ee11f05e4827: /* @__PURE__ */ __name(function(t, e) {
    return t.then(e);
  }, "__wbg_then_8e16ee11f05e4827"), __wbg_toString_6dc1a94e0bdba378: /* @__PURE__ */ __name(function(t) {
    return t.toString();
  }, "__wbg_toString_6dc1a94e0bdba378"), __wbg_url_94ef047be3ab790a: /* @__PURE__ */ __name(function(t, e) {
    let r = e.url, i = h(r, _.__wbindgen_malloc, _.__wbindgen_realloc), c = g;
    w().setInt32(t + 4, c, true), w().setInt32(t + 0, i, true);
  }, "__wbg_url_94ef047be3ab790a"), __wbg_versions_276b2795b1c6a219: /* @__PURE__ */ __name(function(t) {
    return t.versions;
  }, "__wbg_versions_276b2795b1c6a219"), __wbg_view_701664ffb3b1ce67: /* @__PURE__ */ __name(function(t) {
    let e = t.view;
    return b(e) ? 0 : p(e);
  }, "__wbg_view_701664ffb3b1ce67"), __wbg_webSocket_3f14b4f0fc1bdbfa: /* @__PURE__ */ __name(function() {
    return u(function(t) {
      let e = t.webSocket;
      return b(e) ? 0 : p(e);
    }, arguments);
  }, "__wbg_webSocket_3f14b4f0fc1bdbfa"), __wbindgen_cast_0000000000000001: /* @__PURE__ */ __name(function(t, e) {
    return O(t, e, et);
  }, "__wbindgen_cast_0000000000000001"), __wbindgen_cast_0000000000000002: /* @__PURE__ */ __name(function(t, e) {
    return O(t, e, Y);
  }, "__wbindgen_cast_0000000000000002"), __wbindgen_cast_0000000000000003: /* @__PURE__ */ __name(function(t, e) {
    return O(t, e, Z);
  }, "__wbindgen_cast_0000000000000003"), __wbindgen_cast_0000000000000004: /* @__PURE__ */ __name(function(t, e) {
    return O(t, e, tt);
  }, "__wbindgen_cast_0000000000000004"), __wbindgen_cast_0000000000000005: /* @__PURE__ */ __name(function(t, e) {
    return C(t, e);
  }, "__wbindgen_cast_0000000000000005"), __wbindgen_cast_0000000000000006: /* @__PURE__ */ __name(function(t, e) {
    return l(t, e);
  }, "__wbindgen_cast_0000000000000006"), __wbindgen_init_externref_table: /* @__PURE__ */ __name(function() {
    let t = _.__wbindgen_externrefs, e = t.grow(4);
    t.set(0, void 0), t.set(e + 0, void 0), t.set(e + 1, null), t.set(e + 2, true), t.set(e + 3, false);
  }, "__wbindgen_init_externref_table") } };
}
__name(K, "K");
function o() {
  if (Q) {
    U();
    return;
  }
}
__name(o, "o");
function Y(n, t, e) {
  o(), _.wasm_bindgen__convert__closures_____invoke__h5578cc1e20079fda(n, t, e);
}
__name(Y, "Y");
function Z(n, t, e) {
  o(), _.wasm_bindgen__convert__closures_____invoke__h5578cc1e20079fda_2(n, t, e);
}
__name(Z, "Z");
function tt(n, t, e) {
  o(), _.wasm_bindgen__convert__closures_____invoke__h5578cc1e20079fda_3(n, t, e);
}
__name(tt, "tt");
function et(n, t, e) {
  let r;
  if (o(), r = _.wasm_bindgen__convert__closures_____invoke__h736d6d0577a34768(n, t, e), r[1]) throw bt(r[0]);
}
__name(et, "et");
function nt(n, t, e, r) {
  o(), _.wasm_bindgen__convert__closures_____invoke__h6e632cc65fe8f3cb(n, t, e, r);
}
__name(nt, "nt");
var rt = ["bytes"];
var s = 0;
var _t = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_containerstartupoptions_free(n >>> 0, 1);
});
var it = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_intounderlyingbytesource_free(n >>> 0, 1);
});
var ot = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_intounderlyingsink_free(n >>> 0, 1);
});
var st = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_intounderlyingsource_free(n >>> 0, 1);
});
var ct = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_minifyconfig_free(n >>> 0, 1);
});
var V = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_nwcrelay_free(n >>> 0, 1);
});
var at = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry(({ ptr: n, instance: t }) => {
  t === s && _.__wbg_r2range_free(n >>> 0, 1);
});
function p(n) {
  let t = _.__externref_table_alloc();
  return _.__wbindgen_externrefs.set(t, n), t;
}
__name(p, "p");
var B = typeof FinalizationRegistry > "u" ? { register: /* @__PURE__ */ __name(() => {
}, "register"), unregister: /* @__PURE__ */ __name(() => {
}, "unregister") } : new FinalizationRegistry((n) => {
  n.instance === s && _.__wbindgen_destroy_closure(n.a, n.b);
});
function L(n) {
  let t = typeof n;
  if (t == "number" || t == "boolean" || n == null) return `${n}`;
  if (t == "string") return `"${n}"`;
  if (t == "symbol") {
    let i = n.description;
    return i == null ? "Symbol" : `Symbol(${i})`;
  }
  if (t == "function") {
    let i = n.name;
    return typeof i == "string" && i.length > 0 ? `Function(${i})` : "Function";
  }
  if (Array.isArray(n)) {
    let i = n.length, c = "[";
    i > 0 && (c += L(n[0]));
    for (let a = 1; a < i; a++) c += ", " + L(n[a]);
    return c += "]", c;
  }
  let e = /\[object ([^\]]+)\]/.exec(toString.call(n)), r;
  if (e && e.length > 1) r = e[1];
  else return toString.call(n);
  if (r == "Object") try {
    return "Object(" + JSON.stringify(n) + ")";
  } catch {
    return "Object";
  }
  return n instanceof Error ? `${n.name}: ${n.message}
${n.stack}` : r;
}
__name(L, "L");
function ut(n, t) {
  n = n >>> 0;
  let e = w(), r = [];
  for (let i = n; i < n + 4 * t; i += 4) r.push(_.__wbindgen_externrefs.get(e.getUint32(i, true)));
  return _.__externref_drop_slice(n, t), r;
}
__name(ut, "ut");
function C(n, t) {
  return n = n >>> 0, z().subarray(n / 1, n / 1 + t);
}
__name(C, "C");
var v = null;
function w() {
  return (v === null || v.buffer.detached === true || v.buffer.detached === void 0 && v.buffer !== _.memory.buffer) && (v = new DataView(_.memory.buffer)), v;
}
__name(w, "w");
function l(n, t) {
  return n = n >>> 0, wt(n, t);
}
__name(l, "l");
var j = null;
function z() {
  return (j === null || j.byteLength === 0) && (j = new Uint8Array(_.memory.buffer)), j;
}
__name(z, "z");
function u(n, t) {
  try {
    return n.apply(this, t);
  } catch (e) {
    let r = p(e);
    _.__wbindgen_exn_store(r);
  }
}
__name(u, "u");
function b(n) {
  return n == null;
}
__name(b, "b");
function O(n, t, e) {
  let r = { a: n, b: t, cnt: 1, instance: s }, i = /* @__PURE__ */ __name((...c) => {
    if (r.instance !== s) throw new Error("Cannot invoke closure from previous WASM instance");
    r.cnt++;
    let a = r.a;
    r.a = 0;
    try {
      return e(a, r.b, ...c);
    } finally {
      r.a = a, i._wbg_cb_unref();
    }
  }, "i");
  return i._wbg_cb_unref = () => {
    --r.cnt === 0 && (_.__wbindgen_destroy_closure(r.a, r.b), r.a = 0, B.unregister(r));
  }, B.register(i, r, r), i;
}
__name(O, "O");
function ft(n, t) {
  let e = t(n.length * 4, 4) >>> 0;
  for (let r = 0; r < n.length; r++) {
    let i = p(n[r]);
    w().setUint32(e + 4 * r, i, true);
  }
  return g = n.length, e;
}
__name(ft, "ft");
function h(n, t, e) {
  if (e === void 0) {
    let f = P.encode(n), d = t(f.length, 1) >>> 0;
    return z().subarray(d, d + f.length).set(f), g = f.length, d;
  }
  let r = n.length, i = t(r, 1) >>> 0, c = z(), a = 0;
  for (; a < r; a++) {
    let f = n.charCodeAt(a);
    if (f > 127) break;
    c[i + a] = f;
  }
  if (a !== r) {
    a !== 0 && (n = n.slice(a)), i = e(i, r, r = a + n.length * 3, 1) >>> 0;
    let f = z().subarray(i + a, i + r), d = P.encodeInto(n, f);
    a += d.written, i = e(i, r, a, 1) >>> 0;
  }
  return g = a, i;
}
__name(h, "h");
var Q = false;
function bt(n) {
  let t = _.__wbindgen_externrefs.get(n);
  return _.__externref_table_dealloc(n), t;
}
__name(bt, "bt");
var X = new TextDecoder("utf-8", { ignoreBOM: true, fatal: true });
X.decode();
function wt(n, t) {
  return X.decode(z().subarray(n, n + t));
}
__name(wt, "wt");
var P = new TextEncoder();
"encodeInto" in P || (P.encodeInto = function(n, t) {
  let e = P.encode(n);
  return t.set(e), { read: n.length, written: e.length };
});
var g = 0;
var gt = new WebAssembly.Instance($, K());
var _ = gt.exports;
_.__wbindgen_start();
Error.stackTraceLimit = 100;
var y = H();
function D() {
  y.criticalError && (console.log("Reinitializing Wasm application"), U(), y.criticalError = false, y.instanceId++);
}
__name(D, "D");
addEventListener("error", (n) => {
  q(n.error);
});
function q(n) {
  n instanceof WebAssembly.RuntimeError && (console.error("Critical", n), y.criticalError = true);
}
__name(q, "q");
var A = class extends lt {
  static {
    __name(this, "A");
  }
};
A.prototype.fetch = function(t) {
  return J.call(this, t, this.env, this.ctx);
};
A.prototype.init = G;
var pt = { set: /* @__PURE__ */ __name((n, t, e, r) => Reflect.set(n.instance, t, e, r), "set"), has: /* @__PURE__ */ __name((n, t) => Reflect.has(n.instance, t), "has"), deleteProperty: /* @__PURE__ */ __name((n, t) => Reflect.deleteProperty(n.instance, t), "deleteProperty"), apply: /* @__PURE__ */ __name((n, t, e) => Reflect.apply(n.instance, t, e), "apply"), construct: /* @__PURE__ */ __name((n, t, e) => Reflect.construct(n.instance, t, e), "construct"), getPrototypeOf: /* @__PURE__ */ __name((n) => Reflect.getPrototypeOf(n.instance), "getPrototypeOf"), setPrototypeOf: /* @__PURE__ */ __name((n, t) => Reflect.setPrototypeOf(n.instance, t), "setPrototypeOf"), isExtensible: /* @__PURE__ */ __name((n) => Reflect.isExtensible(n.instance), "isExtensible"), preventExtensions: /* @__PURE__ */ __name((n) => Reflect.preventExtensions(n.instance), "preventExtensions"), getOwnPropertyDescriptor: /* @__PURE__ */ __name((n, t) => Reflect.getOwnPropertyDescriptor(n.instance, t), "getOwnPropertyDescriptor"), defineProperty: /* @__PURE__ */ __name((n, t, e) => Reflect.defineProperty(n.instance, t, e), "defineProperty"), ownKeys: /* @__PURE__ */ __name((n) => Reflect.ownKeys(n.instance), "ownKeys") };
var m = { construct(n, t, e) {
  try {
    D();
    let r = { instance: Reflect.construct(n, t, e), instanceId: y.instanceId, ctor: n, args: t, newTarget: e };
    return new Proxy(r, { ...pt, get(i, c, a) {
      i.instanceId !== y.instanceId && (i.instance = Reflect.construct(i.ctor, i.args, i.newTarget), i.instanceId = y.instanceId);
      let f = Reflect.get(i.instance, c, a);
      return typeof f != "function" ? f : f.constructor === Function ? new Proxy(f, { apply(d, T, M) {
        D();
        try {
          return d.apply(T, M);
        } catch (W) {
          throw q(W), W;
        }
      } }) : new Proxy(f, { async apply(d, T, M) {
        D();
        try {
          return await d.apply(T, M);
        } catch (W) {
          throw q(W), W;
        }
      } });
    } });
  } catch (r) {
    throw y.criticalError = true, r;
  }
} };
var It = new Proxy(A, m);
var xt = new Proxy(I, m);
var Et = new Proxy(x, m);
var Rt = new Proxy(E, m);
var St = new Proxy(R, m);
var kt = new Proxy(S, m);
var Ft = new Proxy(k, m);
var Wt = new Proxy(F, m);

// ../../.npm/_npx/32026684e21afda6/node_modules/wrangler/templates/middleware/middleware-ensure-req-body-drained.ts
var drainBody = /* @__PURE__ */ __name(async (request, env, _ctx, middlewareCtx) => {
  try {
    return await middlewareCtx.next(request, env);
  } finally {
    try {
      if (request.body !== null && !request.bodyUsed) {
        const reader = request.body.getReader();
        while (!(await reader.read()).done) {
        }
      }
    } catch (e) {
      console.error("Failed to drain the unused request body.", e);
    }
  }
}, "drainBody");
var middleware_ensure_req_body_drained_default = drainBody;

// ../../.npm/_npx/32026684e21afda6/node_modules/wrangler/templates/middleware/middleware-miniflare3-json-error.ts
function reduceError(e) {
  return {
    name: e?.name,
    message: e?.message ?? String(e),
    stack: e?.stack,
    cause: e?.cause === void 0 ? void 0 : reduceError(e.cause)
  };
}
__name(reduceError, "reduceError");
var jsonError = /* @__PURE__ */ __name(async (request, env, _ctx, middlewareCtx) => {
  try {
    return await middlewareCtx.next(request, env);
  } catch (e) {
    const error = reduceError(e);
    return Response.json(error, {
      status: 500,
      headers: { "MF-Experimental-Error-Stack": "true" }
    });
  }
}, "jsonError");
var middleware_miniflare3_json_error_default = jsonError;

// .wrangler/tmp/bundle-Odd3O6/middleware-insertion-facade.js
var __INTERNAL_WRANGLER_MIDDLEWARE__ = [
  middleware_ensure_req_body_drained_default,
  middleware_miniflare3_json_error_default
];
var middleware_insertion_facade_default = It;

// ../../.npm/_npx/32026684e21afda6/node_modules/wrangler/templates/middleware/common.ts
var __facade_middleware__ = [];
function __facade_register__(...args) {
  __facade_middleware__.push(...args.flat());
}
__name(__facade_register__, "__facade_register__");
function __facade_invokeChain__(request, env, ctx, dispatch, middlewareChain) {
  const [head, ...tail] = middlewareChain;
  const middlewareCtx = {
    dispatch,
    next(newRequest, newEnv) {
      return __facade_invokeChain__(newRequest, newEnv, ctx, dispatch, tail);
    }
  };
  return head(request, env, ctx, middlewareCtx);
}
__name(__facade_invokeChain__, "__facade_invokeChain__");
function __facade_invoke__(request, env, ctx, dispatch, finalMiddleware) {
  return __facade_invokeChain__(request, env, ctx, dispatch, [
    ...__facade_middleware__,
    finalMiddleware
  ]);
}
__name(__facade_invoke__, "__facade_invoke__");

// .wrangler/tmp/bundle-Odd3O6/middleware-loader.entry.ts
var __Facade_ScheduledController__ = class ___Facade_ScheduledController__ {
  constructor(scheduledTime, cron, noRetry) {
    this.scheduledTime = scheduledTime;
    this.cron = cron;
    this.#noRetry = noRetry;
  }
  static {
    __name(this, "__Facade_ScheduledController__");
  }
  #noRetry;
  noRetry() {
    if (!(this instanceof ___Facade_ScheduledController__)) {
      throw new TypeError("Illegal invocation");
    }
    this.#noRetry();
  }
};
function wrapExportedHandler(worker) {
  if (__INTERNAL_WRANGLER_MIDDLEWARE__ === void 0 || __INTERNAL_WRANGLER_MIDDLEWARE__.length === 0) {
    return worker;
  }
  for (const middleware of __INTERNAL_WRANGLER_MIDDLEWARE__) {
    __facade_register__(middleware);
  }
  const fetchDispatcher = /* @__PURE__ */ __name(function(request, env, ctx) {
    if (worker.fetch === void 0) {
      throw new Error("Handler does not export a fetch() function.");
    }
    return worker.fetch(request, env, ctx);
  }, "fetchDispatcher");
  return {
    ...worker,
    fetch(request, env, ctx) {
      const dispatcher = /* @__PURE__ */ __name(function(type, init) {
        if (type === "scheduled" && worker.scheduled !== void 0) {
          const controller = new __Facade_ScheduledController__(
            Date.now(),
            init.cron ?? "",
            () => {
            }
          );
          return worker.scheduled(controller, env, ctx);
        }
      }, "dispatcher");
      return __facade_invoke__(request, env, ctx, dispatcher, fetchDispatcher);
    }
  };
}
__name(wrapExportedHandler, "wrapExportedHandler");
function wrapWorkerEntrypoint(klass) {
  if (__INTERNAL_WRANGLER_MIDDLEWARE__ === void 0 || __INTERNAL_WRANGLER_MIDDLEWARE__.length === 0) {
    return klass;
  }
  for (const middleware of __INTERNAL_WRANGLER_MIDDLEWARE__) {
    __facade_register__(middleware);
  }
  return class extends klass {
    #fetchDispatcher = /* @__PURE__ */ __name((request, env, ctx) => {
      this.env = env;
      this.ctx = ctx;
      if (super.fetch === void 0) {
        throw new Error("Entrypoint class does not define a fetch() function.");
      }
      return super.fetch(request);
    }, "#fetchDispatcher");
    #dispatcher = /* @__PURE__ */ __name((type, init) => {
      if (type === "scheduled" && super.scheduled !== void 0) {
        const controller = new __Facade_ScheduledController__(
          Date.now(),
          init.cron ?? "",
          () => {
          }
        );
        return super.scheduled(controller);
      }
    }, "#dispatcher");
    fetch(request) {
      return __facade_invoke__(
        request,
        this.env,
        this.ctx,
        this.#dispatcher,
        this.#fetchDispatcher
      );
    }
  };
}
__name(wrapWorkerEntrypoint, "wrapWorkerEntrypoint");
var WRAPPED_ENTRY;
if (typeof middleware_insertion_facade_default === "object") {
  WRAPPED_ENTRY = wrapExportedHandler(middleware_insertion_facade_default);
} else if (typeof middleware_insertion_facade_default === "function") {
  WRAPPED_ENTRY = wrapWorkerEntrypoint(middleware_insertion_facade_default);
}
var middleware_loader_entry_default = WRAPPED_ENTRY;
export {
  xt as ContainerStartupOptions,
  Et as IntoUnderlyingByteSource,
  Rt as IntoUnderlyingSink,
  St as IntoUnderlyingSource,
  kt as MinifyConfig,
  Ft as NwcRelay,
  Wt as R2Range,
  __INTERNAL_WRANGLER_MIDDLEWARE__,
  middleware_loader_entry_default as default
};
//# sourceMappingURL=shim.js.map
