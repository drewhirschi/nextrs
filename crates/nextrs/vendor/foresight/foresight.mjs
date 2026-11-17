/*! Vendored by nextrs (scripts/vendor-foresight.sh). Do not edit.
 * js.foresight 4.2.1, position-observer 1.0.3, tabbable 6.5.2 — all MIT.
 * License texts: crates/nextrs/vendor/foresight/LICENSES.md */
var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __typeError = (msg) => {
  throw TypeError(msg);
};
var __defNormalProp = (obj, key, value) => key in obj ? __defProp(obj, key, { enumerable: true, configurable: true, writable: true, value }) : obj[key] = value;
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __publicField = (obj, key, value) => __defNormalProp(obj, typeof key !== "symbol" ? key + "" : key, value);
var __accessCheck = (obj, member, msg) => member.has(obj) || __typeError("Cannot " + msg);
var __privateGet = (obj, member, getter) => (__accessCheck(obj, member, "read from private field"), getter ? getter.call(obj) : member.get(obj));
var __privateAdd = (obj, member, value) => member.has(obj) ? __typeError("Cannot add the same private member more than once") : member instanceof WeakSet ? member.add(obj) : member.set(obj, value);
var __privateSet = (obj, member, value, setter) => (__accessCheck(obj, member, "write to private field"), setter ? setter.call(obj, value) : member.set(obj, value), value);
var __privateMethod = (obj, member, method) => (__accessCheck(obj, member, "access private method"), method);

// node_modules/js.foresight/dist/devLog-CyYTiEHt.mjs
var e;
var init_devLog_CyYTiEHt = __esm({
  "node_modules/js.foresight/dist/devLog-CyYTiEHt.mjs"() {
    e = (e4, t8, n4) => {
      console.log(`%c${e4}: ${t8}`, `color: ${n4}; font-weight: bold;`);
    };
  }
});

// node_modules/js.foresight/dist/rectAndHitSlop-HPBlI01J.mjs
var e2, t, n, r, i, a, o;
var init_rectAndHitSlop_HPBlI01J = __esm({
  "node_modules/js.foresight/dist/rectAndHitSlop-HPBlI01J.mjs"() {
    e2 = 2e3;
    t = 1 / 0;
    n = (e4, t8, n4, r4) => (e4 < t8 ? console.warn(`ForesightJS: "${r4}" value ${e4} is below minimum bound ${t8}, clamping to ${t8}`) : e4 > n4 && console.warn(`ForesightJS: "${r4}" value ${e4} is above maximum bound ${n4}, clamping to ${n4}`), Math.min(Math.max(e4, t8), n4));
    r = (t8) => {
      if (typeof t8 == `number`) {
        let r4 = n(t8, 0, e2, `hitslop`);
        return { top: r4, left: r4, right: r4, bottom: r4 };
      }
      return { top: n(t8.top, 0, e2, `hitslop - top`), left: n(t8.left, 0, e2, `hitslop - left`), right: n(t8.right, 0, e2, `hitslop - right`), bottom: n(t8.bottom, 0, e2, `hitslop - bottom`) };
    };
    i = (e4, t8) => ({ left: e4.left - t8.left, right: e4.right + t8.right, top: e4.top - t8.top, bottom: e4.bottom + t8.bottom });
    a = (e4, t8) => !e4 || !t8 ? e4 === t8 : e4.left === t8.left && e4.right === t8.right && e4.top === t8.top && e4.bottom === t8.bottom;
    o = (e4, t8) => e4.x >= t8.left && e4.x <= t8.right && e4.y >= t8.top && e4.y <= t8.bottom;
  }
});

// node_modules/js.foresight/dist/BaseForesightModule-Bmfn8kzE.mjs
var t2;
var init_BaseForesightModule_Bmfn8kzE = __esm({
  "node_modules/js.foresight/dist/BaseForesightModule-Bmfn8kzE.mjs"() {
    init_devLog_CyYTiEHt();
    t2 = class {
      get isConnected() {
        return this._isConnected;
      }
      constructor(e4) {
        this._isConnected = false, this.elements = e4.elements, this.scannableElements = e4.scannableElements, this.callCallback = e4.callCallback, this.emit = e4.emit, this.hasListeners = e4.hasListeners, this.updateElementState = e4.updateElementState, this.updateElementBounds = e4.updateElementBounds, this.settings = e4.settings;
      }
      disconnect() {
        this.isConnected && (this.devLog(`Disconnecting ${this.moduleName}...`), this.abortController?.abort(`${this.moduleName} module disconnected`), this.onDisconnect(), this._isConnected = false);
      }
      connect() {
        this.isConnected || (this.devLog(`Connecting ${this.moduleName}...`), this.onConnect(), this._isConnected = true);
      }
      devLog(t8) {
        if (!this.settings.enableManagerLogging) return;
        let n4 = this.moduleName.includes(`Predictor`) ? `#ea580c` : `#2563eb`;
        e(this.moduleName, t8, n4);
      }
      createAbortController() {
        this.abortController && !this.abortController.signal.aborted || (this.abortController = new AbortController(), this.devLog(`Created new AbortController for ${this.moduleName}`));
      }
    };
  }
});

// node_modules/js.foresight/dist/ElementObservingModule-oSMdF509.mjs
var t3;
var init_ElementObservingModule_oSMdF509 = __esm({
  "node_modules/js.foresight/dist/ElementObservingModule-oSMdF509.mjs"() {
    init_BaseForesightModule_Bmfn8kzE();
    t3 = class extends t2 {
    };
  }
});

// node_modules/js.foresight/dist/lineSegmentIntersectsRect-qrQZ4bxA.mjs
var e3;
var init_lineSegmentIntersectsRect_qrQZ4bxA = __esm({
  "node_modules/js.foresight/dist/lineSegmentIntersectsRect-qrQZ4bxA.mjs"() {
    e3 = (e4, t8, n4) => {
      let r4 = 0, i3 = 1, a2 = t8.x - e4.x, o3 = t8.y - e4.y, s3 = (e5, t9) => {
        if (e5 === 0) {
          if (t9 < 0) return false;
        } else {
          let n5 = t9 / e5;
          if (e5 < 0) {
            if (n5 > i3) return false;
            n5 > r4 && (r4 = n5);
          } else {
            if (n5 < r4) return false;
            n5 < i3 && (i3 = n5);
          }
        }
        return true;
      };
      return !s3(-a2, e4.x - n4.left) || !s3(a2, n4.right - e4.x) || !s3(-o3, e4.y - n4.top) || !s3(o3, n4.bottom - e4.y) ? false : r4 <= i3;
    };
  }
});

// node_modules/position-observer/dist/index.js
function rootMargin(rect, rootBounds) {
  const width = Math.max(rect.width, MIN_SIZE);
  const height = Math.max(rect.height, MIN_SIZE);
  const top = rect.top - rootBounds.top - INSET;
  const left = rect.left - rootBounds.left - INSET;
  const right = rootBounds.right - rect.left - width - INSET;
  const bottom = rootBounds.bottom - rect.top - height - INSET;
  return `${-Math.round(top)}px ${-Math.round(right)}px ${-Math.round(bottom)}px ${-Math.round(left)}px`;
}
function getWindowRect(window2) {
  const width = window2.visualViewport?.width ?? window2.innerWidth;
  const height = window2.visualViewport?.height ?? window2.innerHeight;
  return new DOMRect(0, 0, width, height);
}
function isNode(target) {
  return "nodeType" in target;
}
function isElement(target) {
  return isNode(target) && target.nodeType === Node.ELEMENT_NODE;
}
function isDocument(root) {
  return root.nodeType === Node.DOCUMENT_NODE;
}
function getRoot(target) {
  return !target || isDocument(target) ? target?.defaultView ?? window : target;
}
function isEntryEqual(first, second) {
  if (second == null) return false;
  return first.target === second.target && first.isIntersecting === second.isIntersecting && Rect.equals(first.boundingClientRect, second.boundingClientRect) && Rect.equals(first.intersectionRect, second.intersectionRect);
}
var Rect, INSET, MIN_SIZE, threshold, _callback, _observer, _options, _clientRect, _previousIntersectionRatio, _PositionIntersectionObserver_instances, observe_fn, _onIntersection, _a, PositionIntersectionObserver, PositionIntersectionObserverEntry, _resizeObserver, _controller, _a2, RootBoundsObserver, _callback2, _observers, _options2, _a3, VisibilityObserver, _callback3, _options3, _positionObservers, _resizeObserver2, _positions, _rootBoundsObserver, _visibilityObserver, _PositionObserver_instances, notify_fn, _onRootBoundsChange, observePosition_fn, _onVisibilityChange, _onPositionChange, _onResize, _a4, PositionObserver, PositionObserverEntry;
var init_dist = __esm({
  "node_modules/position-observer/dist/index.js"() {
    Rect = class {
      /**
       * Intersects two DOMRects.
       *
       * @param rect1 - The first DOMRect.
       * @param rect2 - The second DOMRect.
       * @returns The intersection of the two DOMRects.
       */
      static intersect(rect1, rect2) {
        const left = Math.max(rect1.left, rect2.left);
        const right = Math.min(rect1.right, rect2.right);
        const top = Math.max(rect1.top, rect2.top);
        const bottom = Math.min(rect1.bottom, rect2.bottom);
        const width = Math.max(0, right - left);
        const height = Math.max(0, bottom - top);
        return new DOMRect(left, top, width, height);
      }
      /**
       * Clips a DOMRect to a given rectangle.
       *
       * @param rect - The DOMRect to clip.
       * @param clip - The rectangle to clip the DOMRect to.
       * @returns The clipped DOMRect.
       */
      static clip(rect, clip) {
        const updatedRect = {
          ...rect.toJSON(),
          top: rect.top + clip.top,
          left: rect.left + clip.left,
          bottom: rect.bottom - clip.bottom,
          right: rect.right - clip.right
        };
        updatedRect.width = updatedRect.right - updatedRect.left;
        updatedRect.height = updatedRect.bottom - updatedRect.top;
        return new DOMRect(
          updatedRect.left,
          updatedRect.top,
          updatedRect.width,
          updatedRect.height
        );
      }
      /**
       * Calculates the offsets of a clipped DOMRect.
       *
       * @param rect - The DOMRect to calculate the offsets for.
       * @param clippedRect - The clipped DOMRect.
       * @returns The offsets of the clipped DOMRect.
       */
      static clipOffsets(rect, clippedRect) {
        return {
          top: clippedRect.top - rect.top,
          left: clippedRect.left - rect.left,
          bottom: rect.bottom - clippedRect.bottom,
          right: rect.right - clippedRect.right
        };
      }
      /**
       * Checks if two DOMRects are equal.
       *
       * @param rect1 - The first DOMRect.
       * @param rect2 - The second DOMRect.
       * @returns True if the DOMRects are equal, false otherwise.
       */
      static equals(rect1, rect2) {
        if (rect1 == null || rect2 == null) return rect1 === rect2;
        return rect1.x === rect2.x && rect1.y === rect2.y && rect1.width === rect2.width && rect1.height === rect2.height;
      }
      static sizeEqual(rect1, rect2) {
        return Math.round(rect1.width) === Math.round(rect2.width) && Math.round(rect1.height) === Math.round(rect2.height);
      }
    };
    INSET = -1;
    MIN_SIZE = 1 - INSET * 2;
    threshold = [
      ...Array.from({ length: 1e3 }, (_2, i3) => i3 / 1e3),
      1
    ];
    PositionIntersectionObserver = (_a = class {
      constructor(element, callback, options) {
        __privateAdd(this, _PositionIntersectionObserver_instances);
        __privateAdd(this, _callback);
        __privateAdd(this, _observer);
        __privateAdd(this, _options);
        __privateAdd(this, _clientRect);
        __privateAdd(this, _previousIntersectionRatio);
        /**
         * The callback function for the intersection observer.
         */
        __privateAdd(this, _onIntersection, (entries) => {
          if (!__privateGet(this, _observer)) return;
          const entry = entries[entries.length - 1];
          if (entry) {
            const { intersectionRatio, boundingClientRect } = entry;
            const previousClientRect = __privateGet(this, _clientRect);
            __privateSet(this, _clientRect, boundingClientRect);
            const previousIntersectionRatio = __privateGet(this, _previousIntersectionRatio);
            const clientRectChanged = !Rect.equals(
              boundingClientRect,
              previousClientRect
            );
            if (intersectionRatio !== __privateGet(this, _previousIntersectionRatio) || clientRectChanged) {
              const rootBounds = __privateGet(this, _options).rootBounds;
              const rootIntersection = Rect.intersect(boundingClientRect, rootBounds);
              const isIntersecting = rootIntersection.width > 0 && rootIntersection.height > 0;
              if (!isIntersecting) {
                return;
              }
              __privateSet(this, _previousIntersectionRatio, intersectionRatio);
              if (previousIntersectionRatio != null || clientRectChanged) {
                __privateGet(this, _callback).call(this, new PositionIntersectionObserverEntry(
                  entry.target,
                  boundingClientRect,
                  entry.intersectionRect,
                  isIntersecting,
                  rootBounds
                ), this);
                __privateMethod(this, _PositionIntersectionObserver_instances, observe_fn).call(this, entry.target);
              }
            }
          }
        });
        __privateSet(this, _callback, callback);
        __privateSet(this, _options, options);
        __privateSet(this, _clientRect, options.clientRect);
        __privateMethod(this, _PositionIntersectionObserver_instances, observe_fn).call(this, element);
      }
      /**
       * The visible rectangle of the element within the root.
       *
       * @returns The visible rectangle of the element within the root.
       */
      get visibleRect() {
        const clip = __privateGet(this, _options).clip;
        return clip ? Rect.clip(__privateGet(this, _clientRect), clip) : __privateGet(this, _clientRect);
      }
      /**
       * Whether the element is intersecting with the root.
       *
       * @returns Whether the element is intersecting with the root.
       */
      get isIntersecting() {
        const { width, height } = this.visibleRect;
        return width > 0 && height > 0;
      }
      /**
       * Disconnects the position intersection observer.
       */
      disconnect() {
        __privateGet(this, _observer)?.disconnect();
      }
    }, _callback = new WeakMap(), _observer = new WeakMap(), _options = new WeakMap(), _clientRect = new WeakMap(), _previousIntersectionRatio = new WeakMap(), _PositionIntersectionObserver_instances = new WeakSet(), /**
     * Observes the element.
     */
    observe_fn = function(element) {
      const { root, rootBounds } = __privateGet(this, _options);
      const { visibleRect } = this;
      __privateGet(this, _observer)?.disconnect();
      __privateSet(this, _observer, new IntersectionObserver(__privateGet(this, _onIntersection), {
        root,
        rootMargin: rootMargin(visibleRect, rootBounds),
        threshold
      }));
      __privateGet(this, _observer).observe(element);
    }, _onIntersection = new WeakMap(), _a);
    PositionIntersectionObserverEntry = class {
      constructor(target, boundingClientRect, intersectionRect, isIntersecting, rootBounds) {
        this.target = target;
        this.boundingClientRect = boundingClientRect;
        this.intersectionRect = intersectionRect;
        this.isIntersecting = isIntersecting;
        this.rootBounds = rootBounds;
      }
    };
    RootBoundsObserver = (_a2 = class {
      /**
       * Creates a new root bounds observer.
       *
       * @param root - The root element to observe.
       * @param callback - The callback function to call when the root bounds change.
       */
      constructor(target, callback) {
        /**
         * The resize observer if the root is an element.
         */
        __privateAdd(this, _resizeObserver);
        /**
         * The controller to disconnect the root bounds resize and scroll listeners.
         */
        __privateAdd(this, _controller, new AbortController());
        /**
         * The bounds of the root element.
         */
        __publicField(this, "rootBounds");
        const root = getRoot(target);
        if (isElement(root)) {
          const ownerDocument = root.ownerDocument ?? document;
          this.rootBounds = root.getBoundingClientRect();
          __privateSet(this, _resizeObserver, new ResizeObserver((entries) => {
            for (const entry of entries) {
              const [{ inlineSize: width, blockSize: height }] = entry.borderBoxSize;
              if (Rect.sizeEqual(this.rootBounds, { width, height })) {
                continue;
              }
              const rect = entry.target.getBoundingClientRect();
              this.rootBounds = rect;
              callback(rect, this);
            }
          }));
          __privateGet(this, _resizeObserver).observe(root);
          ownerDocument.addEventListener(
            "scroll",
            (event) => {
              if (event.target && event.target !== root && isNode(event.target) && event.target.contains(root)) {
                this.rootBounds = root.getBoundingClientRect();
                callback(this.rootBounds, this);
              }
            },
            { capture: true, passive: true, signal: __privateGet(this, _controller).signal }
          );
        } else {
          const viewport = root.visualViewport ?? root;
          this.rootBounds = getWindowRect(root);
          const handleResize = () => {
            const rect = getWindowRect(root);
            if (Rect.equals(this.rootBounds, rect)) return;
            this.rootBounds = rect;
            callback(rect, this);
          };
          viewport.addEventListener("resize", handleResize, {
            signal: __privateGet(this, _controller).signal
          });
        }
      }
      /**
       * Disconnects the root bounds observer.
       */
      disconnect() {
        __privateGet(this, _resizeObserver)?.disconnect();
        __privateGet(this, _controller).abort();
      }
    }, _resizeObserver = new WeakMap(), _controller = new WeakMap(), _a2);
    VisibilityObserver = (_a3 = class {
      constructor(callback, options) {
        /**
         * The callback function to be invoked when the intersection entries change.
         */
        __privateAdd(this, _callback2);
        /**
         * The visibility observer for each document.
         */
        __privateAdd(this, _observers, /* @__PURE__ */ new Map());
        /**
         * The options for the visibility observer.
         */
        __privateAdd(this, _options2);
        /**
         * The latest intersection entries for the observed elements.
         */
        __publicField(this, "intersections", /* @__PURE__ */ new WeakMap());
        __privateSet(this, _options2, options);
        __privateSet(this, _callback2, (entries) => {
          const changedEntries = [];
          for (const entry of entries) {
            const previousIntersection = this.intersections.get(entry.target);
            this.intersections.set(entry.target, entry);
            if (previousIntersection?.isIntersecting !== entry.isIntersecting || !Rect.equals(
              previousIntersection?.intersectionRect,
              entry.intersectionRect
            )) {
              changedEntries.push(entry);
            }
          }
          if (changedEntries.length > 0) {
            callback(changedEntries, this);
          }
        });
      }
      /**
       * Observes an element.
       *
       * @param element - The element to observe.
       */
      observe(element) {
        const document2 = element.ownerDocument;
        if (!document2) return;
        let observer = __privateGet(this, _observers).get(document2);
        if (!observer) {
          observer = new IntersectionObserver(__privateGet(this, _callback2), {
            ...__privateGet(this, _options2),
            threshold
          });
          __privateGet(this, _observers).set(document2, observer);
        }
        observer.observe(element);
      }
      /**
       * Unobserves an element.
       *
       * @param element - The element to unobserve.
       */
      unobserve(element) {
        const document2 = element.ownerDocument;
        if (!document2) return;
        const observer = __privateGet(this, _observers).get(document2);
        if (!observer) return;
        observer.unobserve(element);
        this.intersections.delete(element);
      }
      /**
       * Disconnects the visibility observer.
       */
      disconnect() {
        for (const observer of __privateGet(this, _observers).values()) {
          observer.disconnect();
        }
        __privateGet(this, _observers).clear();
      }
    }, _callback2 = new WeakMap(), _observers = new WeakMap(), _options2 = new WeakMap(), _a3);
    PositionObserver = (_a4 = class {
      constructor(callback, options) {
        __privateAdd(this, _PositionObserver_instances);
        /**
         * The callback function to be invoked when the position changes.
         */
        __privateAdd(this, _callback3);
        /**
         * The options for the position observer.
         */
        __privateAdd(this, _options3);
        /**
         * The position observers for the observed elements.
         */
        __privateAdd(this, _positionObservers, /* @__PURE__ */ new Map());
        /**
         * The resize observer for the observed elements.
         */
        __privateAdd(this, _resizeObserver2);
        /**
         * The positions of the observed elements.
         */
        __privateAdd(this, _positions, /* @__PURE__ */ new WeakMap());
        /**
         * The root bounds observer for the observed elements.
         */
        __privateAdd(this, _rootBoundsObserver);
        /**
         * The visibility observer for the observed elements.
         */
        __privateAdd(this, _visibilityObserver);
        /**
         * The callback function to be invoked when the root bounds change.
         */
        __privateAdd(this, _onRootBoundsChange, (rootBounds) => {
          const entries = [];
          for (const [element] of __privateGet(this, _positionObservers)) {
            const boundingClientRect = element.getBoundingClientRect();
            const observer = __privateMethod(this, _PositionObserver_instances, observePosition_fn).call(this, element, boundingClientRect);
            entries.push(
              new PositionObserverEntry(
                element,
                boundingClientRect,
                observer.visibleRect,
                observer.isIntersecting,
                rootBounds
              )
            );
          }
          __privateMethod(this, _PositionObserver_instances, notify_fn).call(this, entries);
        });
        /**
         * The callback function to be invoked when the visibility changes.
         */
        __privateAdd(this, _onVisibilityChange, (entries) => {
          const records = [];
          for (const entry of entries) {
            const { target, isIntersecting, boundingClientRect } = entry;
            if (isIntersecting) {
              __privateMethod(this, _PositionObserver_instances, observePosition_fn).call(this, target, boundingClientRect);
              __privateGet(this, _resizeObserver2).observe(target);
            } else {
              __privateGet(this, _positionObservers).get(target)?.disconnect();
              __privateGet(this, _positionObservers).delete(target);
              __privateGet(this, _resizeObserver2).unobserve(target);
            }
            const observer = __privateGet(this, _positionObservers).get(target);
            records.push(
              new PositionObserverEntry(
                target,
                boundingClientRect,
                observer?.visibleRect ?? entry.intersectionRect,
                isIntersecting,
                __privateGet(this, _rootBoundsObserver).rootBounds
              )
            );
          }
          __privateMethod(this, _PositionObserver_instances, notify_fn).call(this, records);
        });
        /**
         * The callback function to be invoked when the position changes.
         */
        __privateAdd(this, _onPositionChange, (entry, observer) => {
          __privateMethod(this, _PositionObserver_instances, notify_fn).call(this, [
            new PositionObserverEntry(
              entry.target,
              entry.boundingClientRect,
              observer.visibleRect,
              entry.isIntersecting,
              __privateGet(this, _rootBoundsObserver).rootBounds
            )
          ]);
        });
        /**
         * The callback function to be invoked when the resize observer entries change.
         */
        __privateAdd(this, _onResize, (entries) => {
          const records = [];
          for (const entry of entries) {
            const { target, borderBoxSize } = entry;
            const previous = __privateGet(this, _positions).get(target);
            if (previous) {
              const [{ inlineSize: width, blockSize: height }] = borderBoxSize;
              if (Rect.sizeEqual(previous.boundingClientRect, { width, height })) {
                continue;
              }
            }
            const boundingClientRect = target.getBoundingClientRect();
            const observer = __privateMethod(this, _PositionObserver_instances, observePosition_fn).call(this, target, boundingClientRect);
            records.push(
              new PositionObserverEntry(
                target,
                boundingClientRect,
                observer.visibleRect,
                __privateGet(this, _visibilityObserver).intersections.get(target)?.isIntersecting ?? false,
                __privateGet(this, _rootBoundsObserver).rootBounds
              )
            );
          }
          __privateMethod(this, _PositionObserver_instances, notify_fn).call(this, records);
        });
        __privateSet(this, _callback3, callback);
        __privateSet(this, _options3, options);
        __privateSet(this, _rootBoundsObserver, new RootBoundsObserver(
          options?.root,
          __privateGet(this, _onRootBoundsChange)
        ));
        __privateSet(this, _visibilityObserver, new VisibilityObserver(
          __privateGet(this, _onVisibilityChange),
          options
        ));
        __privateSet(this, _resizeObserver2, new ResizeObserver(__privateGet(this, _onResize)));
      }
      /**
       * Observes an element.
       *
       * @param element - The element to observe.
       */
      observe(element) {
        __privateGet(this, _visibilityObserver).observe(element);
      }
      /**
       * Unobserves an element.
       *
       * @param element - The element to unobserve. If not provided, the position observer is disconnected.
       */
      unobserve(element) {
        if (element) {
          __privateGet(this, _positionObservers).get(element)?.disconnect();
          __privateGet(this, _visibilityObserver).unobserve(element);
          __privateGet(this, _positions).delete(element);
        } else {
          this.disconnect();
        }
      }
      /**
       * Disconnects the observer.
       */
      disconnect() {
        for (const positionObserver of __privateGet(this, _positionObservers).values()) {
          positionObserver.disconnect();
        }
        __privateGet(this, _positionObservers).clear();
        __privateGet(this, _resizeObserver2).disconnect();
        __privateGet(this, _rootBoundsObserver).disconnect();
        __privateGet(this, _visibilityObserver).disconnect();
      }
    }, _callback3 = new WeakMap(), _options3 = new WeakMap(), _positionObservers = new WeakMap(), _resizeObserver2 = new WeakMap(), _positions = new WeakMap(), _rootBoundsObserver = new WeakMap(), _visibilityObserver = new WeakMap(), _PositionObserver_instances = new WeakSet(), /**
     * Notifies the position observer of the changes.
     *
     * @param entries - The entries to notify.
     */
    notify_fn = function(entries) {
      const records = [];
      for (const entry of entries) {
        const { target } = entry;
        const previousEntry = __privateGet(this, _positions).get(target);
        if (isEntryEqual(entry, previousEntry)) continue;
        __privateGet(this, _positions).set(target, entry);
        records.push(entry);
      }
      if (records.length > 0) {
        __privateGet(this, _callback3).call(this, records);
      }
    }, _onRootBoundsChange = new WeakMap(), /**
     * Observes the position of an element.
     *
     * @param element - The element to observe.
     * @param clientRect - The client rect of the element.
     */
    observePosition_fn = function(element, clientRect) {
      const visibilityObserver = __privateGet(this, _visibilityObserver);
      __privateGet(this, _positionObservers).get(element)?.disconnect();
      const positionObserver = new PositionIntersectionObserver(
        element,
        __privateGet(this, _onPositionChange),
        {
          clientRect,
          root: __privateGet(this, _options3)?.root,
          rootBounds: __privateGet(this, _rootBoundsObserver).rootBounds,
          get clip() {
            const intersection = visibilityObserver.intersections.get(element);
            if (!intersection) return;
            const { intersectionRect, boundingClientRect } = intersection;
            return Rect.clipOffsets(boundingClientRect, intersectionRect);
          }
        }
      );
      __privateGet(this, _positionObservers).set(element, positionObserver);
      return positionObserver;
    }, _onVisibilityChange = new WeakMap(), _onPositionChange = new WeakMap(), _onResize = new WeakMap(), _a4);
    PositionObserverEntry = class {
      constructor(target, boundingClientRect, intersectionRect, isIntersecting, rootBounds) {
        this.target = target;
        this.boundingClientRect = boundingClientRect;
        this.intersectionRect = intersectionRect;
        this.isIntersecting = isIntersecting;
        this.rootBounds = rootBounds;
      }
    };
  }
});

// node_modules/tabbable/dist/index.esm.js
var candidateSelectors, candidateSelector, NoElement, matches, getRootNode, _isInert, isContentEditable, getCandidates, _getCandidatesIteratively, hasTabIndex, getTabIndex, getSortOrderTabIndex, sortOrderedTabbables, isInput, isHiddenInput, isDetailsWithSummary, getCheckedRadio, isTabbableRadio, isRadio, isNonTabbableRadio, isZeroArea, isHidden, isDisabledFromFieldset, isNodeMatchingSelectorFocusable, isNodeMatchingSelectorTabbable, isShadowRootTabbable, _sortByOrder, tabbable;
var init_index_esm = __esm({
  "node_modules/tabbable/dist/index.esm.js"() {
    candidateSelectors = ["input:not([inert]):not([inert] *)", "select:not([inert]):not([inert] *)", "textarea:not([inert]):not([inert] *)", "a[href]:not([inert]):not([inert] *)", "area[href]:not([inert]):not([inert] *)", "button:not([inert]):not([inert] *)", "[tabindex]:not(slot):not([inert]):not([inert] *)", "audio[controls]:not([inert]):not([inert] *)", "video[controls]:not([inert]):not([inert] *)", '[contenteditable]:not([contenteditable="false"]):not([inert]):not([inert] *)', "details>summary:first-of-type:not([inert]):not([inert] *)", "details:not([inert]):not([inert] *)"];
    candidateSelector = /* @__PURE__ */ candidateSelectors.join(",");
    NoElement = typeof Element === "undefined";
    matches = NoElement ? function() {
    } : Element.prototype.matches || Element.prototype.msMatchesSelector || Element.prototype.webkitMatchesSelector;
    getRootNode = !NoElement && Element.prototype.getRootNode ? function(element) {
      return element == null || element.getRootNode == null ? void 0 : element.getRootNode();
    } : function(element) {
      return element == null ? void 0 : element.ownerDocument;
    };
    _isInert = function isInert(node, lookUp) {
      if (lookUp === void 0) {
        lookUp = true;
      }
      var inertAtt = node == null || node.getAttribute == null ? void 0 : node.getAttribute("inert");
      var inert = inertAtt === "" || inertAtt === "true";
      var result = inert || lookUp && node && // closest does not exist on shadow roots, so we fall back to a manual
      // lookup upward, in case it is not defined.
      (typeof node.closest === "function" ? node.closest("[inert]") : _isInert(node.parentNode));
      return result;
    };
    isContentEditable = function isContentEditable2(node) {
      var _node$getAttribute;
      var attValue = node == null || node.getAttribute == null || (_node$getAttribute = node.getAttribute("contenteditable")) == null ? void 0 : _node$getAttribute.toLowerCase();
      return attValue === "" || attValue === "true" || attValue === "plaintext-only";
    };
    getCandidates = function getCandidates2(el, includeContainer, filter) {
      if (_isInert(el)) {
        return [];
      }
      var candidates = Array.prototype.slice.apply(el.querySelectorAll(candidateSelector));
      if (includeContainer && matches.call(el, candidateSelector)) {
        candidates.unshift(el);
      }
      candidates = candidates.filter(filter);
      return candidates;
    };
    _getCandidatesIteratively = function getCandidatesIteratively(elements, includeContainer, options) {
      var candidates = [];
      var elementsToCheck = Array.from(elements);
      while (elementsToCheck.length) {
        var element = elementsToCheck.shift();
        if (_isInert(element, false)) {
          continue;
        }
        if (element.tagName === "SLOT") {
          var assigned = element.assignedElements();
          var content = assigned.length ? assigned : element.children;
          var nestedCandidates = _getCandidatesIteratively(content, true, options);
          if (options.flatten) {
            candidates.push.apply(candidates, nestedCandidates);
          } else {
            candidates.push({
              scopeParent: element,
              candidates: nestedCandidates
            });
          }
        } else {
          var validCandidate = matches.call(element, candidateSelector);
          if (validCandidate && options.filter(element) && (includeContainer || !elements.includes(element))) {
            candidates.push(element);
          }
          var shadowRoot = element.shadowRoot || // check for an undisclosed shadow
          typeof options.getShadowRoot === "function" && options.getShadowRoot(element);
          var validShadowRoot = !_isInert(shadowRoot, false) && (!options.shadowRootFilter || options.shadowRootFilter(element));
          if (shadowRoot && validShadowRoot) {
            var _nestedCandidates = _getCandidatesIteratively(shadowRoot === true ? element.children : shadowRoot.children, true, options);
            if (options.flatten) {
              candidates.push.apply(candidates, _nestedCandidates);
            } else {
              candidates.push({
                scopeParent: element,
                candidates: _nestedCandidates
              });
            }
          } else {
            elementsToCheck.unshift.apply(elementsToCheck, element.children);
          }
        }
      }
      return candidates;
    };
    hasTabIndex = function hasTabIndex2(node) {
      return !isNaN(parseInt(node.getAttribute("tabindex"), 10));
    };
    getTabIndex = function getTabIndex2(node) {
      if (!node) {
        throw new Error("No node provided");
      }
      if (node.tabIndex < 0) {
        if ((/^(AUDIO|VIDEO|DETAILS)$/.test(node.tagName) || isContentEditable(node)) && !hasTabIndex(node)) {
          return 0;
        }
      }
      return node.tabIndex;
    };
    getSortOrderTabIndex = function getSortOrderTabIndex2(node, isScope) {
      var tabIndex = getTabIndex(node);
      if (tabIndex < 0 && isScope && !hasTabIndex(node)) {
        return 0;
      }
      return tabIndex;
    };
    sortOrderedTabbables = function sortOrderedTabbables2(a2, b2) {
      return a2.tabIndex === b2.tabIndex ? a2.documentOrder - b2.documentOrder : a2.tabIndex - b2.tabIndex;
    };
    isInput = function isInput2(node) {
      return node.tagName === "INPUT";
    };
    isHiddenInput = function isHiddenInput2(node) {
      return isInput(node) && node.type === "hidden";
    };
    isDetailsWithSummary = function isDetailsWithSummary2(node) {
      var r4 = node.tagName === "DETAILS" && Array.prototype.slice.apply(node.children).some(function(child) {
        return child.tagName === "SUMMARY";
      });
      return r4;
    };
    getCheckedRadio = function getCheckedRadio2(nodes, form) {
      for (var i3 = 0; i3 < nodes.length; i3++) {
        if (nodes[i3].checked && nodes[i3].form === form) {
          return nodes[i3];
        }
      }
    };
    isTabbableRadio = function isTabbableRadio2(node) {
      if (!node.name) {
        return true;
      }
      var radioScope = node.form || getRootNode(node);
      var queryRadios = function queryRadios2(name) {
        return radioScope.querySelectorAll('input[type="radio"][name="' + name + '"]');
      };
      var radioSet;
      if (typeof window !== "undefined" && typeof window.CSS !== "undefined" && typeof window.CSS.escape === "function") {
        radioSet = queryRadios(window.CSS.escape(node.name));
      } else {
        try {
          radioSet = queryRadios(node.name);
        } catch (err) {
          console.error("Looks like you have a radio button with a name attribute containing invalid CSS selector characters and need the CSS.escape polyfill: %s", err.message);
          return false;
        }
      }
      var checked = getCheckedRadio(radioSet, node.form);
      return !checked || checked === node;
    };
    isRadio = function isRadio2(node) {
      return isInput(node) && node.type === "radio";
    };
    isNonTabbableRadio = function isNonTabbableRadio2(node) {
      return isRadio(node) && !isTabbableRadio(node);
    };
    isZeroArea = function isZeroArea2(node) {
      var _node$getBoundingClie = node.getBoundingClientRect(), width = _node$getBoundingClie.width, height = _node$getBoundingClie.height;
      return width === 0 && height === 0;
    };
    isHidden = function isHidden2(node, _ref) {
      var displayCheck = _ref.displayCheck, getShadowRoot = _ref.getShadowRoot;
      if (displayCheck === "full-native") {
        if ("checkVisibility" in node) {
          var visible = node.checkVisibility({
            // Checking opacity might be desirable for some use cases, but natively,
            // opacity zero elements _are_ focusable and tabbable.
            checkOpacity: false,
            opacityProperty: false,
            contentVisibilityAuto: true,
            visibilityProperty: true,
            // This is an alias for `visibilityProperty`. Contemporary browsers
            // support both. However, this alias has wider browser support (Chrome
            // >= 105 and Firefox >= 106, vs. Chrome >= 121 and Firefox >= 122), so
            // we include it anyway.
            checkVisibilityCSS: true
          });
          return !visible;
        }
      }
      var _getComputedStyle = getComputedStyle(node), visibility = _getComputedStyle.visibility;
      if (visibility === "hidden" || visibility === "collapse") {
        return true;
      }
      var isDirectSummary = matches.call(node, "details>summary:first-of-type");
      var nodeUnderDetails = isDirectSummary ? node.parentElement : node;
      if (matches.call(nodeUnderDetails, "details:not([open]) *")) {
        return true;
      }
      if (!displayCheck || displayCheck === "full" || // full-native can run this branch when it falls through in case
      // Element#checkVisibility is unsupported
      displayCheck === "full-native" || displayCheck === "legacy-full") {
        if (typeof getShadowRoot === "function") {
          var originalNode = node;
          while (node) {
            var parentElement = node.parentElement;
            var rootNode = getRootNode(node);
            if (parentElement && !parentElement.shadowRoot && getShadowRoot(parentElement) === true) {
              return isZeroArea(node);
            } else if (node.assignedSlot) {
              node = node.assignedSlot;
            } else if (!parentElement && rootNode !== node.ownerDocument) {
              node = rootNode.host;
            } else {
              node = parentElement;
            }
          }
          node = originalNode;
        }
        if (node.isConnected) {
          return !node.getClientRects().length;
        }
        if (displayCheck !== "legacy-full") {
          return true;
        }
      } else if (displayCheck === "non-zero-area") {
        return isZeroArea(node);
      }
      return false;
    };
    isDisabledFromFieldset = function isDisabledFromFieldset2(node) {
      if (/^(INPUT|BUTTON|SELECT|TEXTAREA)$/.test(node.tagName)) {
        var parentNode = node.parentElement;
        while (parentNode) {
          if (parentNode.tagName === "FIELDSET" && parentNode.disabled) {
            for (var i3 = 0; i3 < parentNode.children.length; i3++) {
              var child = parentNode.children.item(i3);
              if (child.tagName === "LEGEND") {
                return matches.call(parentNode, "fieldset[disabled] *") ? true : !child.contains(node);
              }
            }
            return true;
          }
          parentNode = parentNode.parentElement;
        }
      }
      return false;
    };
    isNodeMatchingSelectorFocusable = function isNodeMatchingSelectorFocusable2(options, node) {
      if (node.disabled || isHiddenInput(node) || isHidden(node, options) || // For a details element with a summary, the summary element gets the focus
      isDetailsWithSummary(node) || isDisabledFromFieldset(node)) {
        return false;
      }
      return true;
    };
    isNodeMatchingSelectorTabbable = function isNodeMatchingSelectorTabbable2(options, node) {
      if (isNonTabbableRadio(node) || getTabIndex(node) < 0 || !isNodeMatchingSelectorFocusable(options, node)) {
        return false;
      }
      return true;
    };
    isShadowRootTabbable = function isShadowRootTabbable2(shadowHostNode) {
      var tabIndex = parseInt(shadowHostNode.getAttribute("tabindex"), 10);
      if (isNaN(tabIndex) || tabIndex >= 0) {
        return true;
      }
      return false;
    };
    _sortByOrder = function sortByOrder(candidates) {
      var regularTabbables = [];
      var orderedTabbables = [];
      candidates.forEach(function(item, i3) {
        var isScope = !!item.scopeParent;
        var element = isScope ? item.scopeParent : item;
        var candidateTabindex = getSortOrderTabIndex(element, isScope);
        var elements = isScope ? _sortByOrder(item.candidates) : element;
        if (candidateTabindex === 0) {
          isScope ? regularTabbables.push.apply(regularTabbables, elements) : regularTabbables.push(element);
        } else {
          orderedTabbables.push({
            documentOrder: i3,
            tabIndex: candidateTabindex,
            item,
            isScope,
            content: elements
          });
        }
      });
      return orderedTabbables.sort(sortOrderedTabbables).reduce(function(acc, sortable) {
        sortable.isScope ? acc.push.apply(acc, sortable.content) : acc.push(sortable.content);
        return acc;
      }, []).concat(regularTabbables);
    };
    tabbable = function tabbable2(container, options) {
      options = options || {};
      var candidates;
      if (options.getShadowRoot) {
        candidates = _getCandidatesIteratively([container], options.includeContainer, {
          filter: isNodeMatchingSelectorTabbable.bind(null, options),
          flatten: false,
          getShadowRoot: options.getShadowRoot,
          shadowRootFilter: isShadowRootTabbable
        });
      } else {
        candidates = getCandidates(container, options.includeContainer, isNodeMatchingSelectorTabbable.bind(null, options));
      }
      return _sortByOrder(candidates);
    };
  }
});

// node_modules/js.foresight/dist/TabPredictor-BLaHx9Pa.mjs
var TabPredictor_BLaHx9Pa_exports = {};
__export(TabPredictor_BLaHx9Pa_exports, {
  TabPredictor: () => r2
});
var n2, r2;
var init_TabPredictor_BLaHx9Pa = __esm({
  "node_modules/js.foresight/dist/TabPredictor-BLaHx9Pa.mjs"() {
    init_BaseForesightModule_Bmfn8kzE();
    init_index_esm();
    n2 = (e4, t8, n4, r4) => {
      if (t8 !== null && t8 > -1) {
        let i3 = e4 ? t8 - 1 : t8 + 1;
        if (i3 >= 0 && i3 < n4.length && n4[i3] === r4) return i3;
      }
      return n4.findIndex((e5) => e5 === r4);
    };
    r2 = class extends t2 {
      constructor(e4) {
        super(e4), this.moduleName = `TabPredictor`, this.lastKeyDown = null, this.tabbableElementsCache = [], this.lastFocusedIndex = null, this.handleKeyDown = (e5) => {
          e5.key === `Tab` && (this.lastKeyDown = e5);
        }, this.handleFocusIn = (e5) => {
          if (!this.lastKeyDown || !(e5.target instanceof HTMLElement)) return;
          let t8 = this.lastKeyDown.shiftKey;
          this.lastKeyDown = null, this.ensureTabbableCache();
          let r4 = n2(t8, this.lastFocusedIndex, this.tabbableElementsCache, e5.target);
          this.lastFocusedIndex = r4, this.predictUpcomingTabs(r4, t8);
        };
      }
      invalidateCache() {
        this.tabbableElementsCache.length && this.devLog(`Invalidating tabbable elements cache`), this.tabbableElementsCache = [], this.lastFocusedIndex = null;
      }
      onConnect() {
        typeof document > `u` || (this.createAbortController(), document.addEventListener(`keydown`, this.handleKeyDown, { signal: this.abortController?.signal, passive: true }), document.addEventListener(`focusin`, this.handleFocusIn, { signal: this.abortController?.signal, passive: true }));
      }
      onDisconnect() {
        this.tabbableElementsCache = [], this.lastFocusedIndex = null, this.lastKeyDown = null;
      }
      ensureTabbableCache() {
        this.tabbableElementsCache.length && this.lastFocusedIndex !== -1 || (this.devLog(`Caching tabbable elements`), this.tabbableElementsCache = tabbable(document.documentElement));
      }
      predictUpcomingTabs(e4, t8) {
        let n4 = this.settings.tabOffset, r4 = t8 ? `reverse` : `forwards`;
        for (let i3 = 0; i3 <= n4; i3++) {
          let n5 = t8 ? e4 - i3 : e4 + i3, a2 = this.tabbableElementsCache[n5];
          if (!(a2 instanceof Element)) continue;
          let o3 = this.elements.get(a2);
          o3 && !o3.state.isPredicted && o3.state.isActive && this.callCallback(o3, { kind: `tab`, subType: r4 });
        }
      }
    };
  }
});

// node_modules/js.foresight/dist/ScrollPredictor-BgEEpjMT.mjs
var ScrollPredictor_BgEEpjMT_exports = {};
__export(ScrollPredictor_BgEEpjMT_exports, {
  ScrollPredictor: () => i2
});
var n3, r3, i2;
var init_ScrollPredictor_BgEEpjMT = __esm({
  "node_modules/js.foresight/dist/ScrollPredictor-BgEEpjMT.mjs"() {
    init_BaseForesightModule_Bmfn8kzE();
    init_lineSegmentIntersectsRect_qrQZ4bxA();
    n3 = (e4, t8) => {
      let n4 = t8.top - e4.top, r4 = t8.left - e4.left;
      return n4 < -1 ? `down` : n4 > 1 ? `up` : r4 < -1 ? `right` : r4 > 1 ? `left` : `none`;
    };
    r3 = (e4, t8, n4, r4) => {
      switch (r4.x = e4.x, r4.y = e4.y, t8) {
        case `down`:
          r4.y += n4;
          break;
        case `up`:
          r4.y -= n4;
          break;
        case `left`:
          r4.x -= n4;
          break;
        case `right`:
          r4.x += n4;
          break;
        case `none`:
          break;
        default:
      }
    };
    i2 = class extends t2 {
      constructor(e4) {
        super(e4.dependencies), this.moduleName = `ScrollPredictor`, this.predictedScrollPoint = { x: 0, y: 0 }, this.hasPredictedScrollPoint = false, this.scrollDirection = null, this.onDisconnect = () => this.resetScrollProps(), this.trajectoryPositions = e4.trajectoryPositions, this.scrollTrajectoryEvent = { type: `scrollTrajectoryUpdate`, currentPoint: this.trajectoryPositions.currentPoint, predictedPoint: { x: 0, y: 0 }, scrollDirection: `none` };
      }
      resetScrollProps() {
        this.scrollDirection = null, this.hasPredictedScrollPoint = false;
      }
      handleScrollPrefetch(e4, i3) {
        let a2 = e4.state;
        if (!(!a2.isIntersectingWithViewport || a2.isPredicted || !a2.isActive)) {
          if (!this.scrollDirection) {
            let t8 = n3(e4.bounds.originalRect, i3);
            if (t8 === `none`) return;
            this.scrollDirection = t8;
          }
          this.hasPredictedScrollPoint || (this.hasPredictedScrollPoint = (r3(this.trajectoryPositions.currentPoint, this.scrollDirection, this.settings.scrollMargin, this.predictedScrollPoint), true)), e3(this.trajectoryPositions.currentPoint, this.predictedScrollPoint, e4.bounds.expandedRect) && this.callCallback(e4, { kind: `scroll`, subType: this.scrollDirection });
        }
      }
      emitTrajectoryUpdate() {
        !this.scrollDirection || !this.hasPredictedScrollPoint || !this.hasListeners(`scrollTrajectoryUpdate`) || (this.scrollTrajectoryEvent.predictedPoint = this.predictedScrollPoint, this.scrollTrajectoryEvent.scrollDirection = this.scrollDirection, this.emit(this.scrollTrajectoryEvent));
      }
      onConnect() {
      }
    };
  }
});

// node_modules/js.foresight/dist/DesktopHandler-CuMJcOud.mjs
var DesktopHandler_CuMJcOud_exports = {};
__export(DesktopHandler_CuMJcOud_exports, {
  DesktopHandler: () => u
});
var s, c, l, u;
var init_DesktopHandler_CuMJcOud = __esm({
  "node_modules/js.foresight/dist/DesktopHandler-CuMJcOud.mjs"() {
    init_rectAndHitSlop_HPBlI01J();
    init_BaseForesightModule_Bmfn8kzE();
    init_ElementObservingModule_oSMdF509();
    init_lineSegmentIntersectsRect_qrQZ4bxA();
    init_dist();
    s = (e4, t8, n4, r4) => {
      let i3 = performance.now(), a2 = t8.peekWriteSlot();
      a2 ? (a2.point.x = e4.x, a2.point.y = e4.y, a2.time = i3, t8.add(a2)) : t8.add({ point: { x: e4.x, y: e4.y }, time: i3 });
      let { x: o3, y: s3 } = e4;
      if (t8.length < 2) {
        r4.x = o3, r4.y = s3;
        return;
      }
      let c3 = t8.getFirst(), l3 = t8.getLast();
      if (!c3 || !l3) {
        r4.x = o3, r4.y = s3;
        return;
      }
      let u3 = (l3.time - c3.time) * 1e-3;
      if (u3 === 0) {
        r4.x = o3, r4.y = s3;
        return;
      }
      let d2 = l3.point.x - c3.point.x, f2 = l3.point.y - c3.point.y, p2 = d2 / u3, m2 = f2 / u3, h = n4 * 1e-3;
      r4.x = o3 + p2 * h, r4.y = s3 + m2 * h;
    };
    c = class extends t2 {
      constructor(e4) {
        super(e4.dependencies), this.moduleName = `MousePredictor`, this.trajectoryPositions = e4.trajectoryPositions, this.mouseTrajectoryEvent = { type: `mouseTrajectoryUpdate`, predictionEnabled: false, trajectoryPositions: this.trajectoryPositions };
      }
      updatePointerState(e4) {
        let t8 = this.trajectoryPositions.currentPoint;
        t8.x = e4.clientX, t8.y = e4.clientY, this.settings.enableMousePrediction ? s(t8, this.trajectoryPositions.positions, this.settings.trajectoryPredictionTime, this.trajectoryPositions.predictedPoint) : (this.trajectoryPositions.predictedPoint.x = t8.x, this.trajectoryPositions.predictedPoint.y = t8.y);
      }
      processMouseMovement(e4) {
        this.updatePointerState(e4);
        let n4 = this.settings.enableMousePrediction, r4 = this.trajectoryPositions.currentPoint;
        for (let e5 of this.scannableElements) {
          let i3 = e5.bounds.expandedRect;
          if (n4) e3(r4, this.trajectoryPositions.predictedPoint, i3) && this.callCallback(e5, { kind: `mouse`, subType: `trajectory` });
          else if (o(r4, i3)) {
            this.callCallback(e5, { kind: `mouse`, subType: `hover` });
            continue;
          }
        }
        this.hasListeners(`mouseTrajectoryUpdate`) && (this.mouseTrajectoryEvent.predictionEnabled = n4, this.emit(this.mouseTrajectoryEvent));
      }
      onDisconnect() {
      }
      onConnect() {
      }
    };
    l = class {
      constructor(e4) {
        if (this.head = 0, this.count = 0, e4 <= 0) throw Error(`CircularBuffer capacity must be greater than 0`);
        this.capacity = e4, this.buffer = Array(e4);
      }
      add(e4) {
        this.buffer[this.head] = e4, this.head = (this.head + 1) % this.capacity, this.count < this.capacity && this.count++;
      }
      peekWriteSlot() {
        return this.buffer[this.head];
      }
      getFirst() {
        if (this.count !== 0) return this.count < this.capacity ? this.buffer[0] : this.buffer[this.head];
      }
      getLast() {
        if (this.count !== 0) {
          if (this.count < this.capacity) return this.buffer[this.count - 1];
          {
            let e4 = (this.head - 1 + this.capacity) % this.capacity;
            return this.buffer[e4];
          }
        }
      }
      resize(e4) {
        if (e4 <= 0) throw Error(`CircularBuffer capacity must be greater than 0`);
        if (e4 === this.capacity) return;
        let t8 = this.getAllItems();
        if (this.capacity = e4, this.buffer = Array(e4), this.head = 0, this.count = 0, t8.length > e4) {
          let n4 = t8.slice(-e4);
          for (let e5 of n4) this.add(e5);
        } else for (let e5 of t8) this.add(e5);
      }
      getAllItems() {
        if (this.count === 0) return [];
        let e4 = Array(this.count);
        if (this.count < this.capacity) for (let t8 = 0; t8 < this.count; t8++) e4[t8] = this.buffer[t8];
        else {
          let t8 = this.head;
          for (let n4 = 0; n4 < this.capacity; n4++) {
            let r4 = (t8 + n4) % this.capacity;
            e4[n4] = this.buffer[r4];
          }
        }
        return e4;
      }
      get length() {
        return this.count;
      }
    };
    u = class extends t3 {
      constructor(r4) {
        super(r4), this.moduleName = `DesktopHandler`, this.tabPredictor = null, this.scrollPredictor = null, this.positionObserver = null, this.trajectoryPositions = { positions: new l(this.settings.positionHistorySize), currentPoint: { x: 0, y: 0 }, predictedPoint: { x: 0, y: 0 } }, this.handlePositionChange = (e4) => {
          let t8 = this.settings.enableScrollPrediction;
          for (let n4 of e4) {
            let e5 = this.elements.get(n4.target);
            e5 && (t8 ? this.scrollPredictor?.handleScrollPrefetch(e5, n4.boundingClientRect) : this.checkForMouseHover(e5), this.handlePositionChangeDataUpdates(e5, n4));
          }
          t8 && (this.scrollPredictor?.emitTrajectoryUpdate(), this.scrollPredictor?.resetScrollProps());
        }, this.checkForMouseHover = (e4) => {
          o(this.trajectoryPositions.currentPoint, e4.bounds.expandedRect) && this.callCallback(e4, { kind: `mouse`, subType: `hover` });
        }, this.handlePositionChangeDataUpdates = (t8, r5) => {
          let i3 = r5.isIntersecting;
          i3 && !a(r5.boundingClientRect, t8.bounds.originalRect) && this.updateElementBounds(t8, { originalRect: r5.boundingClientRect, expandedRect: i(r5.boundingClientRect, t8.state.hitSlop) }), t8.state.isIntersectingWithViewport !== i3 && this.updateElementState(t8, { isIntersectingWithViewport: i3 });
        }, this.processMouseMovement = (e4) => this.mousePredictor.processMouseMovement(e4), this.invalidateTabCache = () => this.tabPredictor?.invalidateCache(), this.connectTabPredictor = async () => {
          if (!this.tabPredictor) {
            let { TabPredictor: e4 } = await Promise.resolve().then(() => (init_TabPredictor_BLaHx9Pa(), TabPredictor_BLaHx9Pa_exports));
            this.tabPredictor = new e4(this.storedDependencies), this.devLog(`TabPredictor lazy loaded`);
          }
          this.isConnected && this.tabPredictor.connect();
        }, this.connectScrollPredictor = async () => {
          if (!this.scrollPredictor) {
            let { ScrollPredictor: e4 } = await Promise.resolve().then(() => (init_ScrollPredictor_BgEEpjMT(), ScrollPredictor_BgEEpjMT_exports));
            this.scrollPredictor = new e4({ dependencies: this.storedDependencies, trajectoryPositions: this.trajectoryPositions }), this.devLog(`ScrollPredictor lazy loaded`);
          }
          this.isConnected && this.scrollPredictor.connect();
        }, this.connectMousePredictor = () => this.mousePredictor.connect(), this.disconnectTabPredictor = () => this.tabPredictor?.disconnect(), this.disconnectScrollPredictor = () => this.scrollPredictor?.disconnect(), this.disconnectMousePredictor = () => this.mousePredictor.disconnect(), this.storedDependencies = r4, this.mousePredictor = new c({ dependencies: r4, trajectoryPositions: this.trajectoryPositions });
      }
      onConnect() {
        this.settings.enableTabPrediction && this.connectTabPredictor(), this.settings.enableScrollPrediction && this.connectScrollPredictor(), this.connectMousePredictor(), this.positionObserver = new PositionObserver(this.handlePositionChange);
        let e4 = [`mouse`];
        this.settings.enableTabPrediction && e4.push(`tab (loading...)`), this.settings.enableScrollPrediction && e4.push(`scroll (loading...)`), this.devLog(`Connected predictors: [${e4.join(`, `)}] and PositionObserver`);
        for (let [e5, t8] of this.elements) t8.state.isActive && this.positionObserver.observe(e5);
      }
      onDisconnect() {
        this.disconnectMousePredictor(), this.disconnectTabPredictor(), this.disconnectScrollPredictor(), this.positionObserver?.disconnect(), this.positionObserver = null;
      }
      observeElement(e4) {
        this.positionObserver?.observe(e4);
      }
      unobserveElement(e4) {
        this.positionObserver?.unobserve(e4);
      }
      get loadedPredictors() {
        return { mouse: true, tab: this.tabPredictor !== null, scroll: this.scrollPredictor !== null };
      }
    };
  }
});

// node_modules/js.foresight/dist/ViewportPredictor-DcLaSz6-.mjs
var ViewportPredictor_DcLaSz6_exports = {};
__export(ViewportPredictor_DcLaSz6_exports, {
  ViewportPredictor: () => t4
});
var t4;
var init_ViewportPredictor_DcLaSz6 = __esm({
  "node_modules/js.foresight/dist/ViewportPredictor-DcLaSz6-.mjs"() {
    init_ElementObservingModule_oSMdF509();
    t4 = class extends t3 {
      constructor(e4) {
        super(e4), this.moduleName = `ViewportPredictor`, this.intersectionObserver = null, this.onConnect = () => this.intersectionObserver = new IntersectionObserver(this.handleViewportEnter), this.handleViewportEnter = (e5) => {
          for (let t8 of e5) {
            if (!t8.isIntersecting) continue;
            let e6 = this.elements.get(t8.target);
            e6 && (this.callCallback(e6, { kind: `viewport` }), this.unobserveElement(t8.target));
          }
        };
      }
      onDisconnect() {
        this.intersectionObserver?.disconnect(), this.intersectionObserver = null;
      }
      observeElement(e4) {
        this.intersectionObserver?.observe(e4);
      }
      unobserveElement(e4) {
        this.intersectionObserver?.unobserve(e4);
      }
    };
  }
});

// node_modules/js.foresight/dist/TouchStartPredictor-BwfbVsZA.mjs
var TouchStartPredictor_BwfbVsZA_exports = {};
__export(TouchStartPredictor_BwfbVsZA_exports, {
  TouchStartPredictor: () => t5
});
var t5;
var init_TouchStartPredictor_BwfbVsZA = __esm({
  "node_modules/js.foresight/dist/TouchStartPredictor-BwfbVsZA.mjs"() {
    init_ElementObservingModule_oSMdF509();
    t5 = class extends t3 {
      constructor(e4) {
        super(e4), this.moduleName = `TouchStartPredictor`, this.onConnect = () => this.createAbortController(), this.onDisconnect = () => {
        }, this.handleTouchStart = (e5) => {
          let t8 = e5.currentTarget, n4 = this.elements.get(t8);
          n4 && (this.callCallback(n4, { kind: `touch` }), this.unobserveElement(t8));
        };
      }
      observeElement(e4) {
        e4 instanceof HTMLElement && e4.addEventListener(`pointerdown`, this.handleTouchStart, { signal: this.abortController?.signal });
      }
      unobserveElement(e4) {
        e4 instanceof HTMLElement && e4.removeEventListener(`pointerdown`, this.handleTouchStart);
      }
    };
  }
});

// node_modules/js.foresight/dist/TouchDeviceHandler-BOjTn6rF.mjs
var TouchDeviceHandler_BOjTn6rF_exports = {};
__export(TouchDeviceHandler_BOjTn6rF_exports, {
  TouchDeviceHandler: () => t6
});
var t6;
var init_TouchDeviceHandler_BOjTn6rF = __esm({
  "node_modules/js.foresight/dist/TouchDeviceHandler-BOjTn6rF.mjs"() {
    init_ElementObservingModule_oSMdF509();
    t6 = class extends t3 {
      constructor(e4) {
        super(e4), this.moduleName = `TouchDeviceHandler`, this.viewportPredictor = null, this.touchStartPredictor = null, this.predictor = null, this.onDisconnect = () => {
          this.devLog(`Disconnecting touch predictor`), this.predictor?.disconnect();
        }, this.onConnect = () => this.setTouchPredictor(), this.storedDependencies = e4;
      }
      async getOrCreateViewportPredictor() {
        if (!this.viewportPredictor) {
          let { ViewportPredictor: e4 } = await Promise.resolve().then(() => (init_ViewportPredictor_DcLaSz6(), ViewportPredictor_DcLaSz6_exports));
          this.viewportPredictor = new e4(this.storedDependencies), this.devLog(`ViewportPredictor lazy loaded`);
        }
        return this.viewportPredictor;
      }
      async getOrCreateTouchStartPredictor() {
        if (!this.touchStartPredictor) {
          let { TouchStartPredictor: e4 } = await Promise.resolve().then(() => (init_TouchStartPredictor_BwfbVsZA(), TouchStartPredictor_BwfbVsZA_exports));
          this.touchStartPredictor = new e4(this.storedDependencies), this.devLog(`TouchStartPredictor lazy loaded`);
        }
        return this.touchStartPredictor;
      }
      async setTouchPredictor() {
        switch (this.predictor?.disconnect(), this.settings.touchDeviceStrategy) {
          case `viewport`:
            this.predictor = await this.getOrCreateViewportPredictor(), this.devLog(`Connected touch strategy: ${this.settings.touchDeviceStrategy} (ViewportPredictor)`);
            break;
          case `onTouchStart`:
            this.predictor = await this.getOrCreateTouchStartPredictor(), this.devLog(`Connected touch strategy: ${this.settings.touchDeviceStrategy} (TouchStartPredictor)`);
            break;
          case `none`:
            this.predictor = null, this.devLog(`Touch strategy set to "none" - no predictor connected`);
            return;
          default:
            this.settings.touchDeviceStrategy;
        }
        if (this.isConnected) {
          this.predictor?.connect();
          for (let [e4, t8] of this.elements) t8.state.isActive && this.predictor?.observeElement(e4);
        }
      }
      observeElement(e4) {
        this.predictor?.observeElement(e4);
      }
      unobserveElement(e4) {
        this.predictor?.unobserveElement(e4);
      }
      get loadedPredictors() {
        return { viewport: this.viewportPredictor !== null, touchStart: this.touchStartPredictor !== null };
      }
    };
  }
});

// node_modules/js.foresight/dist/index.mjs
init_devLog_CyYTiEHt();
init_rectAndHitSlop_HPBlI01J();
var o2 = class {
  constructor(e4, t8) {
    this.source = e4, this.derive = t8;
  }
  get size() {
    return this.source.size;
  }
  get(e4) {
    let t8 = this.source.get(e4);
    return t8 === void 0 ? void 0 : this.derive(t8);
  }
  has(e4) {
    return this.source.has(e4);
  }
  forEach(e4) {
    this.source.forEach((t8, n4) => e4(this.derive(t8), n4, this));
  }
  *entries() {
    for (let [e4, t8] of this.source) yield [e4, this.derive(t8)];
  }
  *values() {
    for (let e4 of this.source.values()) yield this.derive(e4);
  }
  keys() {
    return this.source.keys();
  }
  [Symbol.iterator]() {
    return this.entries();
  }
  get [Symbol.toStringTag]() {
    return `DerivedMapView`;
  }
};
var s2 = () => typeof window > `u` || typeof navigator > `u` ? false : window.matchMedia(`(pointer: coarse)`).matches && navigator.maxTouchPoints > 0;
var c2 = (e4) => {
  let t8 = navigator.connection;
  if (!t8) return false;
  let n4 = [`slow-2g`, `2g`, `3g`, `4g`];
  return n4.indexOf(t8.effectiveType) < n4.indexOf(e4) || t8.saveData;
};
var l2 = (e4) => {
  if (typeof window > `u` || typeof document > `u`) return false;
  let t8 = window.innerWidth || document.documentElement.clientWidth, n4 = window.innerHeight || document.documentElement.clientHeight;
  return e4.top < n4 && e4.bottom > 0 && e4.left < t8 && e4.right > 0;
};
var u2 = () => ({ mouse: { hover: 0, trajectory: 0 }, tab: { forwards: 0, reverse: 0 }, scroll: { down: 0, left: 0, right: 0, up: 0 }, touch: 0, viewport: 0, total: 0 });
var d = () => ({ enableManagerLogging: false, enableMousePrediction: true, enableScrollPrediction: true, positionHistorySize: 8, trajectoryPredictionTime: 120, scrollMargin: 150, defaultHitSlop: { top: 0, left: 0, right: 0, bottom: 0 }, enableTabPrediction: true, tabOffset: 2, touchDeviceStrategy: `onTouchStart`, minimumConnectionType: `3g`, setDataAttributes: true });
var f = Object.freeze({});
var p = (e4) => ({ id: ``, name: ``, meta: f, hitSlop: { top: 0, left: 0, right: 0, bottom: 0 }, isLimitedConnection: e4, isIntersectingWithViewport: false, isRegistered: false, isActive: false, isParked: false, isEnabled: false, isPredicted: false, isCallbackRunning: false, hitCount: 0, registerCount: 0, durationMs: void 0, status: void 0, error: null, reactivateAfter: t });
var m = (e4, t8, i3, a2) => {
  let { element: o3, callback: s3, hitSlop: c3, name: u3, meta: d2, reactivateAfter: m2, enabled: h } = e4, g2 = o3.getBoundingClientRect(), _2 = c3 === void 0 ? i3 : r(c3), v2 = h !== false;
  return { state: { ...p(a2), id: t8, name: u3 || o3.id || `unnamed`, meta: d2 ?? f, hitSlop: _2, isIntersectingWithViewport: l2(g2), isRegistered: true, isActive: v2 && !a2, isEnabled: v2, registerCount: 1, reactivateAfter: m2 ?? 1 / 0 }, bounds: { originalRect: g2, expandedRect: i(g2, _2) }, invokedAt: void 0, completedAt: void 0, element: o3, callback: s3, reactivateTimeoutId: void 0, subscribers: void 0, boundsSubscribers: void 0 };
};
var g = { "data-predicted": (e4) => e4.isPredicted, "data-active": (e4) => e4.isActive, "data-callback-running": (e4) => e4.isCallbackRunning, "data-status": (e4) => e4.status };
var _ = Object.entries(g);
var v = Object.keys(g);
var y = (e4, t8) => {
  for (let [n4, r4] of _) {
    let i3 = r4(t8);
    typeof i3 == `string` ? e4.setAttribute(n4, i3) : e4.toggleAttribute(n4, i3 === true);
  }
};
var b = (e4) => {
  for (let t8 of v) e4.removeAttribute(t8);
};
var x = (e4) => e4.isIntersectingWithViewport && e4.isActive && !e4.isPredicted;
var S = class {
  constructor() {
    this.eventListeners = /* @__PURE__ */ new Map();
  }
  addEventListener(e4, t8, n4) {
    if (n4?.signal?.aborted) return;
    let r4 = this.eventListeners.get(e4) ?? /* @__PURE__ */ new Set();
    r4.add(t8), this.eventListeners.set(e4, r4), n4?.signal?.addEventListener(`abort`, () => this.removeEventListener(e4, t8));
  }
  removeEventListener(e4, t8) {
    this.eventListeners.get(e4)?.delete(t8);
  }
  emit(e4) {
    let t8 = this.eventListeners.get(e4.type);
    if (!(!t8 || t8.size === 0)) for (let n4 of t8) try {
      n4(e4);
    } catch (t9) {
      console.error(`Error in ForesightManager event listener for ${e4.type}:`, t9);
    }
  }
  hasListeners(e4) {
    let t8 = this.eventListeners.get(e4);
    return t8 !== void 0 && t8.size > 0;
  }
  getEventListeners() {
    return this.eventListeners;
  }
};
var C = { trajectoryPredictionTime: { min: 10, max: 200 }, positionHistorySize: { min: 2, max: 30 }, scrollMargin: { min: 30, max: 300 }, tabOffset: { min: 0, max: 20 } };
var w = (e4, t8) => e4 !== void 0 && t8 !== e4;
var T = (e4, n4, r4) => {
  if (!w(r4, e4[n4])) return false;
  let { min: i3, max: a2 } = C[n4];
  return e4[n4] = n(r4, i3, a2, n4), true;
};
var E = (e4, t8, n4) => w(n4, e4[t8]) ? (e4[t8] = n4, true) : false;
var D = (e4, t8) => {
  let r4 = [];
  if (!t8) return r4;
  for (let n4 of [`trajectoryPredictionTime`, `positionHistorySize`, `scrollMargin`, `tabOffset`]) {
    let i3 = e4[n4];
    T(e4, n4, t8[n4]) && r4.push({ setting: n4, oldValue: i3, newValue: e4[n4] });
  }
  for (let n4 of [`enableMousePrediction`, `enableScrollPrediction`, `enableTabPrediction`, `enableManagerLogging`, `setDataAttributes`]) {
    let i3 = e4[n4];
    E(e4, n4, t8[n4]) && r4.push({ setting: n4, oldValue: i3, newValue: e4[n4] });
  }
  if (t8.defaultHitSlop !== void 0) {
    let i3 = e4.defaultHitSlop, o3 = r(t8.defaultHitSlop);
    a(i3, o3) || (e4.defaultHitSlop = o3, r4.push({ setting: `defaultHitSlop`, oldValue: i3, newValue: o3 }));
  }
  if (w(t8.touchDeviceStrategy, e4.touchDeviceStrategy)) {
    let n4 = e4.touchDeviceStrategy;
    e4.touchDeviceStrategy = t8.touchDeviceStrategy, r4.push({ setting: `touchDeviceStrategy`, oldValue: n4, newValue: t8.touchDeviceStrategy });
  }
  if (w(t8.minimumConnectionType, e4.minimumConnectionType)) {
    let n4 = e4.minimumConnectionType;
    e4.minimumConnectionType = t8.minimumConnectionType, r4.push({ setting: `minimumConnectionType`, oldValue: n4, newValue: t8.minimumConnectionType });
  }
  return r4;
};
var O = class t7 {
  constructor(e4) {
    this.elementEntries = /* @__PURE__ */ new Map(), this.scannableElements = /* @__PURE__ */ new Set(), this.registeredElements = new o2(this.elementEntries, (e5) => e5.state), this.idCounter = 0, this.desktopHandler = null, this.touchDeviceHandler = null, this.currentlyActiveHandler = null, this.isSetup = false, this.pendingPointerEvent = null, this.rafId = null, this.domObserver = null, this.currentDeviceStrategy = s2() ? `touch` : `mouse`, this.eventEmitter = new S(), this._globalCallbackHits = u2(), this._globalSettings = d(), this.handlePointerMove = (e5) => {
      this.pendingPointerEvent = e5, e5.pointerType !== this.currentDeviceStrategy && (this.eventEmitter.emit({ type: `deviceStrategyChanged`, timestamp: Date.now(), newStrategy: e5.pointerType, oldStrategy: this.currentDeviceStrategy }), this.currentDeviceStrategy = e5.pointerType, this.setDeviceStrategy(this.currentDeviceStrategy)), !this.rafId && (this.rafId = requestAnimationFrame(() => {
        if (!this.isUsingDesktopHandler) {
          this.rafId = null;
          return;
        }
        this.pendingPointerEvent && this.desktopHandler?.processMouseMovement(this.pendingPointerEvent), this.rafId = null;
      }));
    }, this.handleDomMutations = (e5) => {
      if (!e5.length) return;
      this.desktopHandler?.invalidateTabCache();
      let t8 = false;
      for (let n4 = 0; n4 < e5.length; n4++) {
        let r4 = e5[n4];
        if (r4 && r4.type === `childList` && (r4.removedNodes.length > 0 || r4.addedNodes.length > 0)) {
          t8 = true;
          break;
        }
      }
      if (t8) for (let e6 of this.elementEntries.values()) e6.state.isParked ? e6.element.isConnected && this.resumeReconnected(e6) : e6.element.isConnected || this.parkDisconnected(e6);
    }, D(this._globalSettings, e4), this.handlerDependencies = { elements: this.elementEntries, scannableElements: this.scannableElements, callCallback: this.callCallback.bind(this), emit: this.eventEmitter.emit.bind(this.eventEmitter), hasListeners: this.eventEmitter.hasListeners.bind(this.eventEmitter), updateElementState: this.updateElementState.bind(this), updateElementBounds: this.updateElementBounds.bind(this), settings: this._globalSettings }, this.devLog(`ForesightManager initialized with device strategy: ${this.currentDeviceStrategy}`), this.initializeGlobalListeners();
  }
  async getOrCreateDesktopHandler() {
    if (!this.desktopHandler) {
      let { DesktopHandler: e4 } = await Promise.resolve().then(() => (init_DesktopHandler_CuMJcOud(), DesktopHandler_CuMJcOud_exports));
      this.desktopHandler = new e4(this.handlerDependencies), this.devLog(`DesktopHandler lazy loaded`);
    }
    return this.desktopHandler;
  }
  async getOrCreateTouchHandler() {
    if (!this.touchDeviceHandler) {
      let { TouchDeviceHandler: e4 } = await Promise.resolve().then(() => (init_TouchDeviceHandler_BOjTn6rF(), TouchDeviceHandler_BOjTn6rF_exports));
      this.touchDeviceHandler = new e4(this.handlerDependencies), this.devLog(`TouchDeviceHandler lazy loaded`);
    }
    return this.touchDeviceHandler;
  }
  static initialize(e4) {
    return this.isInitiated || (t7.manager = new t7(e4)), t7.manager;
  }
  static get isInitiated() {
    return !!t7.manager;
  }
  static get instance() {
    return this.initialize();
  }
  generateId() {
    return `foresight-${++this.idCounter}`;
  }
  get isUsingDesktopHandler() {
    return this.currentDeviceStrategy === `mouse` || this.currentDeviceStrategy === `pen`;
  }
  get activeElementCount() {
    let e4 = 0;
    for (let t8 of this.elementEntries.values()) t8.state.isActive && e4++;
    return e4;
  }
  get parkedElementCount() {
    let e4 = 0;
    for (let t8 of this.elementEntries.values()) t8.state.isParked && e4++;
    return e4;
  }
  addEventListener(e4, t8, n4) {
    this.eventEmitter.addEventListener(e4, t8, n4);
  }
  removeEventListener(e4, t8) {
    this.eventEmitter.removeEventListener(e4, t8);
  }
  hasListeners(e4) {
    return this.eventEmitter.hasListeners(e4);
  }
  subscribeToElement(e4, t8) {
    let n4 = this.elementEntries.get(e4);
    if (n4) return this.makeSubscribe(n4, `subscribers`)(t8);
  }
  subscribeToElementBounds(e4, t8) {
    let n4 = this.elementEntries.get(e4);
    if (n4) return this.makeSubscribe(n4, `boundsSubscribers`)(t8);
  }
  getElementBounds(e4) {
    return this.elementEntries.get(e4)?.bounds;
  }
  get getManagerData() {
    return { registeredElements: this.registeredElements, globalSettings: this._globalSettings, globalCallbackHits: this._globalCallbackHits, eventListeners: this.eventEmitter.getEventListeners(), currentDeviceStrategy: this.currentDeviceStrategy, activeElementCount: this.activeElementCount, parkedElementCount: this.parkedElementCount, loadedModules: this.getLoadedModulesSnapshot() };
  }
  getLoadedModulesSnapshot() {
    let e4 = this.desktopHandler?.loadedPredictors, t8 = this.touchDeviceHandler?.loadedPredictors;
    return { desktopHandler: this.desktopHandler !== null, touchHandler: this.touchDeviceHandler !== null, predictors: { mouse: e4?.mouse ?? false, tab: e4?.tab ?? false, scroll: e4?.scroll ?? false, viewport: t8?.viewport ?? false, touchStart: t8?.touchStart ?? false } };
  }
  register(e4) {
    let { element: t8, ...n4 } = e4;
    return t8 instanceof NodeList ? Array.from(t8, (e5) => this.registerElement({ ...n4, element: e5 })) : this.registerElement({ ...n4, element: t8 });
  }
  registerElement(e4) {
    let t8 = c2(this._globalSettings.minimumConnectionType), n4 = this.elementEntries.get(e4.element);
    if (n4) return this.updateElementOptions(e4.element, e4), this.updateElementState(n4, { registerCount: n4.state.registerCount + 1 }), this.buildRegisterResult(n4, e4.element);
    this.initializeGlobalListeners();
    let r4 = m(e4, this.generateId(), this._globalSettings.defaultHitSlop, t8);
    return this.elementEntries.set(e4.element, r4), x(r4.state) && this.scannableElements.add(r4), this._globalSettings.setDataAttributes && y(r4.element, r4.state), r4.state.isActive && this.currentlyActiveHandler?.observeElement(e4.element), this.eventEmitter.emit({ type: `elementRegistered`, timestamp: Date.now(), element: e4.element, state: r4.state }), this.buildRegisterResult(r4, e4.element);
  }
  buildRegisterResult(e4, t8) {
    return { ...e4.state, unregister: () => {
      this.unregister(t8);
    }, subscribe: this.makeSubscribe(e4, `subscribers`), getSnapshot: () => e4.state, subscribeToBounds: this.makeSubscribe(e4, `boundsSubscribers`), getBounds: () => e4.bounds };
  }
  updateElementOptions(e4, t8) {
    let i3 = this.elementEntries.get(e4);
    if (!i3) throw Error(`Cannot update options: element is not registered.`);
    t8.callback && (i3.callback = t8.callback), t8.enabled !== void 0 && this.setElementEnabled(i3, e4, t8.enabled !== false);
    let o3 = i3.state.hitSlop;
    if (t8.hitSlop !== void 0) {
      let s4 = r(t8.hitSlop);
      if (!a(s4, o3)) {
        o3 = s4;
        let t9 = e4.getBoundingClientRect();
        this.updateElementBounds(i3, { originalRect: t9, expandedRect: i(t9, o3) });
      }
    }
    let s3 = i3.state.reactivateAfter, c3 = t8.reactivateAfter ?? s3, l3 = this.updateElementState(i3, { name: t8.name || i3.state.name, meta: t8.meta ?? i3.state.meta, reactivateAfter: c3, hitSlop: o3 });
    return c3 !== s3 && (i3.reactivateTimeoutId !== void 0 && this.clearReactivateTimeout(i3), c3 !== 1 / 0 && l3.isPredicted && this.scheduleReactivateTimeout(i3, c3)), l3;
  }
  makeSubscribe(e4, t8) {
    return (n4) => ((e4[t8] ?? (e4[t8] = /* @__PURE__ */ new Set())).add(n4), () => {
      e4[t8]?.delete(n4);
    });
  }
  updateElementState(e4, t8) {
    let n4 = e4.state, r4 = false;
    for (let e5 in t8) if (t8[e5] !== n4[e5]) {
      r4 = true;
      break;
    }
    if (!r4) return n4;
    let i3 = { ...n4, ...t8 };
    if (e4.state = i3, x(i3) ? this.scannableElements.add(e4) : this.scannableElements.delete(e4), this._globalSettings.setDataAttributes && y(e4.element, i3), e4.subscribers) for (let t9 of e4.subscribers) try {
      t9();
    } catch (e5) {
      console.error(`Error in element subscriber for ${i3.name}:`, e5);
    }
    return i3;
  }
  updateElementBounds(e4, t8) {
    let n4 = e4.bounds;
    if (a(t8.originalRect, n4.originalRect) && a(t8.expandedRect, n4.expandedRect)) return n4;
    if (e4.bounds = t8, e4.boundsSubscribers) for (let t9 of e4.boundsSubscribers) try {
      t9();
    } catch (t10) {
      console.error(`Error in element bounds subscriber for ${e4.state.name}:`, t10);
    }
    return t8;
  }
  unregister(e4, t8) {
    e4 instanceof NodeList ? e4.forEach((e5) => this.unregisterElement(e5, t8)) : this.unregisterElement(e4, t8);
  }
  unregisterElement(e4, t8) {
    let n4 = this.elementEntries.get(e4);
    if (!n4) return;
    this.clearReactivateTimeout(n4), this.currentlyActiveHandler?.unobserveElement(e4);
    let r4 = this.updateElementState(n4, { isRegistered: false, isActive: false, isParked: false, isPredicted: false, isCallbackRunning: false });
    this.elementEntries.delete(e4), this.scannableElements.delete(n4), n4.subscribers?.clear(), n4.boundsSubscribers?.clear(), this._globalSettings.setDataAttributes && b(e4);
    let i3 = this.elementEntries.size === 0 && this.isSetup;
    i3 && (this.devLog(`All elements unregistered, removing global listeners`), this.removeGlobalListeners()), this.eventEmitter.emit({ type: `elementUnregistered`, element: e4, state: r4, timestamp: Date.now(), unregisterReason: t8 ?? `by user`, wasLastRegisteredElement: i3 });
  }
  reactivate(e4) {
    e4 instanceof NodeList ? e4.forEach((e5) => this.reactivateElement(e5)) : this.reactivateElement(e4);
  }
  reactivateElement(e4) {
    let t8 = this.elementEntries.get(e4);
    !t8 || !t8.state.isEnabled || (this.initializeGlobalListeners(), this.clearReactivateTimeout(t8), !(t8.state.isCallbackRunning || t8.state.isActive) && (this.updateElementState(t8, { isActive: true, isPredicted: false }), this.currentlyActiveHandler?.observeElement(e4)));
  }
  setElementEnabled(e4, t8, n4) {
    if (e4.state.isEnabled === n4) return;
    let r4 = n4 && !e4.state.isLimitedConnection && !e4.state.isParked;
    r4 ? (this.initializeGlobalListeners(), this.currentlyActiveHandler?.observeElement(t8)) : (this.clearReactivateTimeout(e4), this.currentlyActiveHandler?.unobserveElement(t8)), this.updateElementState(e4, { isEnabled: n4, isActive: r4, isPredicted: false, isCallbackRunning: false }), this.removeGlobalListenersIfIdle();
  }
  clearReactivateTimeout(e4) {
    clearTimeout(e4.reactivateTimeoutId), e4.reactivateTimeoutId = void 0;
  }
  scheduleReactivateTimeout(e4, t8) {
    e4.reactivateTimeoutId = setTimeout(() => {
      this.reactivate(e4.element);
    }, t8);
  }
  callCallback(e4, t8) {
    e4.state.isPredicted || !e4.state.isActive || (this.markElementAsRunning(e4), this.executeCallbackAsync(e4, t8));
  }
  markElementAsRunning(e4) {
    this.clearReactivateTimeout(e4), e4.invokedAt = Date.now(), this.updateElementState(e4, { isPredicted: true, isCallbackRunning: true, hitCount: e4.state.hitCount + 1 });
  }
  async executeCallbackAsync(e4, t8) {
    this.updateHitCounters(t8), this.eventEmitter.hasListeners(`callbackInvoked`) && this.eventEmitter.emit({ type: `callbackInvoked`, timestamp: Date.now(), element: e4.element, state: e4.state, hitType: t8 });
    let n4 = performance.now(), r4 = null;
    try {
      await e4.callback(e4.state);
    } catch (t9) {
      r4 = t9 instanceof Error ? t9.message : String(t9), console.error(`Error in callback for element ${e4.state.name}:`, t9);
    }
    let i3 = r4 === null ? `success` : `error`;
    this.finalizeCallback(e4, t8, n4, i3, r4);
  }
  finalizeCallback(e4, t8, n4, r4, i3) {
    let a2 = performance.now() - n4;
    this.currentlyActiveHandler?.unobserveElement(e4.element), e4.completedAt = Date.now();
    let o3 = this.updateElementState(e4, { isCallbackRunning: false, isActive: false, durationMs: a2, status: r4, error: i3 });
    o3.reactivateAfter !== 1 / 0 && this.scheduleReactivateTimeout(e4, o3.reactivateAfter), this.removeGlobalListenersIfIdle(), this.eventEmitter.hasListeners(`callbackCompleted`) && this.eventEmitter.emit({ type: `callbackCompleted`, timestamp: Date.now(), element: e4.element, state: o3, hitType: t8, elapsed: a2, status: r4, errorMessage: i3, wasLastActiveElement: this.activeElementCount === 0 });
  }
  updateHitCounters(e4) {
    switch (e4.kind) {
      case `mouse`:
        this._globalCallbackHits.mouse[e4.subType]++;
        break;
      case `tab`:
        this._globalCallbackHits.tab[e4.subType]++;
        break;
      case `scroll`:
        this._globalCallbackHits.scroll[e4.subType]++;
        break;
      case `touch`:
        this._globalCallbackHits.touch++;
        break;
      case `viewport`:
        this._globalCallbackHits.viewport++;
        break;
      default:
    }
    this._globalCallbackHits.total++;
  }
  async setDeviceStrategy(e4) {
    let t8 = this.currentDeviceStrategy;
    t8 !== e4 && this.devLog(`Switching device strategy from ${t8} to ${e4}`), this.currentlyActiveHandler?.disconnect(), this.currentlyActiveHandler = e4 === `mouse` || e4 === `pen` ? await this.getOrCreateDesktopHandler() : await this.getOrCreateTouchHandler(), this.currentlyActiveHandler.connect();
  }
  initializeGlobalListeners() {
    this.isSetup || typeof document > `u` || (this.devLog(`Initializing global listeners (pointermove, MutationObserver)`), this.setDeviceStrategy(this.currentDeviceStrategy), document.addEventListener(`pointermove`, this.handlePointerMove, { passive: true }), this.domObserver = new MutationObserver(this.handleDomMutations), this.domObserver.observe(document.documentElement, { childList: true, subtree: true, attributes: false }), this.isSetup = true);
  }
  removeGlobalListeners() {
    typeof document > `u` || (this.isSetup = false, this.domObserver?.disconnect(), this.domObserver = null, document.removeEventListener(`pointermove`, this.handlePointerMove), this.currentlyActiveHandler?.disconnect(), this.rafId && (this.rafId = (cancelAnimationFrame(this.rafId), null)), this.pendingPointerEvent = null);
  }
  parkDisconnected(e4) {
    this.clearReactivateTimeout(e4), this.currentlyActiveHandler?.unobserveElement(e4.element), this.updateElementState(e4, { isActive: false, isCallbackRunning: false, isParked: true });
  }
  resumeReconnected(e4) {
    let t8 = e4.state.isEnabled && !e4.state.isLimitedConnection, n4 = t8 && !e4.state.isPredicted;
    n4 && (this.initializeGlobalListeners(), this.currentlyActiveHandler?.observeElement(e4.element)), this.updateElementState(e4, { isActive: n4, isParked: false }), t8 && e4.state.isPredicted && e4.state.reactivateAfter !== 1 / 0 && this.scheduleReactivateTimeout(e4, e4.state.reactivateAfter);
  }
  removeGlobalListenersIfIdle() {
    this.activeElementCount > 0 || this.parkedElementCount > 0 || this.removeGlobalListeners();
  }
  alterGlobalSettings(e4) {
    let t8 = D(this._globalSettings, e4);
    if (t8.length === 0) return;
    let n4 = new Set(t8.map((e5) => e5.setting));
    if (n4.has(`positionHistorySize`) && this.desktopHandler && this.desktopHandler.trajectoryPositions.positions.resize(this._globalSettings.positionHistorySize), n4.has(`enableScrollPrediction`) && this.isUsingDesktopHandler && this.desktopHandler && (this._globalSettings.enableScrollPrediction ? this.desktopHandler.connectScrollPredictor() : this.desktopHandler.disconnectScrollPredictor()), n4.has(`enableTabPrediction`) && this.isUsingDesktopHandler && this.desktopHandler && (this._globalSettings.enableTabPrediction ? this.desktopHandler.connectTabPredictor() : this.desktopHandler.disconnectTabPredictor()), n4.has(`defaultHitSlop`) && this.forceUpdateAllElementBounds(), n4.has(`setDataAttributes`)) {
      let e5 = this._globalSettings.setDataAttributes;
      for (let t9 of this.elementEntries.values()) e5 ? y(t9.element, t9.state) : b(t9.element);
    }
    n4.has(`touchDeviceStrategy`) && !this.isUsingDesktopHandler && this.touchDeviceHandler && this.touchDeviceHandler.setTouchPredictor(), this.eventEmitter.emit({ type: `managerSettingsChanged`, timestamp: Date.now(), managerData: this.getManagerData, updatedSettings: t8 });
  }
  forceUpdateAllElementBounds() {
    for (let e4 of this.elementEntries.values()) e4.state.isIntersectingWithViewport && this.forceUpdateElementBounds(e4);
  }
  forceUpdateElementBounds(e4) {
    let t8 = e4.element.getBoundingClientRect();
    this.updateElementBounds(e4, { originalRect: t8, expandedRect: i(t8, e4.state.hitSlop) });
  }
  devLog(t8) {
    this._globalSettings.enableManagerLogging && e(`\u{1F6E0}\uFE0F ForesightManager`, t8, `#16a34a`);
  }
};
export {
  O as ForesightManager
};
