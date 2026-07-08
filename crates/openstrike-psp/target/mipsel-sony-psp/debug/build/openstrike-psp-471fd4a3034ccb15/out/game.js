(() => {
  // vendor/pocketjs/node_modules/solid-js/dist/solid.js
  var sharedConfig = {
    context: undefined,
    registry: undefined,
    effects: undefined,
    done: false,
    getContextId() {
      return getContextId(this.context.count);
    },
    getNextContextId() {
      return getContextId(this.context.count++);
    }
  };
  function getContextId(count) {
    const num = String(count), len = num.length - 1;
    return sharedConfig.context.id + (len ? String.fromCharCode(96 + len) : "") + num;
  }
  function setHydrateContext(context) {
    sharedConfig.context = context;
  }
  function nextHydrateContext() {
    return {
      ...sharedConfig.context,
      id: sharedConfig.getNextContextId(),
      count: 0
    };
  }
  var IS_DEV = false;
  var equalFn = (a, b) => a === b;
  var $PROXY = Symbol("solid-proxy");
  var SUPPORTS_PROXY = typeof Proxy === "function";
  var $TRACK = Symbol("solid-track");
  var $DEVCOMP = Symbol("solid-dev-component");
  var signalOptions = {
    equals: equalFn
  };
  var ERROR = null;
  var runEffects = runQueue;
  var STALE = 1;
  var PENDING = 2;
  var UNOWNED = {
    owned: null,
    cleanups: null,
    context: null,
    owner: null
  };
  var Owner = null;
  var Transition = null;
  var Scheduler = null;
  var ExternalSourceConfig = null;
  var Listener = null;
  var Updates = null;
  var Effects = null;
  var ExecCount = 0;
  function createRoot(fn, detachedOwner) {
    const listener = Listener, owner = Owner, unowned = fn.length === 0, current = detachedOwner === undefined ? owner : detachedOwner, root = unowned ? UNOWNED : {
      owned: null,
      cleanups: null,
      context: current ? current.context : null,
      owner: current
    }, updateFn = unowned ? fn : () => fn(() => untrack(() => cleanNode(root)));
    Owner = root;
    Listener = null;
    try {
      return runUpdates(updateFn, true);
    } finally {
      Listener = listener;
      Owner = owner;
    }
  }
  function createSignal(value, options) {
    options = options ? Object.assign({}, signalOptions, options) : signalOptions;
    const s = {
      value,
      observers: null,
      observerSlots: null,
      comparator: options.equals || undefined
    };
    const setter = (value2) => {
      if (typeof value2 === "function") {
        if (Transition && Transition.running && Transition.sources.has(s))
          value2 = value2(s.tValue);
        else
          value2 = value2(s.value);
      }
      return writeSignal(s, value2);
    };
    return [readSignal.bind(s), setter];
  }
  function createRenderEffect(fn, value, options) {
    const c = createComputation(fn, value, false, STALE);
    if (Scheduler && Transition && Transition.running)
      Updates.push(c);
    else
      updateComputation(c);
  }
  function createMemo(fn, value, options) {
    options = options ? Object.assign({}, signalOptions, options) : signalOptions;
    const c = createComputation(fn, value, true, 0);
    c.observers = null;
    c.observerSlots = null;
    c.comparator = options.equals || undefined;
    if (Scheduler && Transition && Transition.running) {
      c.tState = STALE;
      Updates.push(c);
    } else
      updateComputation(c);
    return readSignal.bind(c);
  }
  function untrack(fn) {
    if (!ExternalSourceConfig && Listener === null)
      return fn();
    const listener = Listener;
    Listener = null;
    try {
      if (ExternalSourceConfig)
        return ExternalSourceConfig.untrack(fn);
      return fn();
    } finally {
      Listener = listener;
    }
  }
  function onCleanup(fn) {
    if (Owner === null)
      ;
    else if (Owner.cleanups === null)
      Owner.cleanups = [fn];
    else
      Owner.cleanups.push(fn);
    return fn;
  }
  function startTransition(fn) {
    if (Transition && Transition.running) {
      fn();
      return Transition.done;
    }
    const l = Listener;
    const o = Owner;
    return Promise.resolve().then(() => {
      Listener = l;
      Owner = o;
      let t;
      if (Scheduler || SuspenseContext) {
        t = Transition || (Transition = {
          sources: new Set,
          effects: [],
          promises: new Set,
          disposed: new Set,
          queue: new Set,
          running: true
        });
        t.done || (t.done = new Promise((res) => t.resolve = res));
        t.running = true;
      }
      runUpdates(fn, false);
      Listener = Owner = null;
      return t ? t.done : undefined;
    });
  }
  var [transPending, setTransPending] = /* @__PURE__ */ createSignal(false);
  var SuspenseContext;
  function readSignal() {
    const runningTransition = Transition && Transition.running;
    if (this.sources && (runningTransition ? this.tState : this.state)) {
      if ((runningTransition ? this.tState : this.state) === STALE)
        updateComputation(this);
      else {
        const updates = Updates;
        Updates = null;
        runUpdates(() => lookUpstream(this), false);
        Updates = updates;
      }
    }
    if (Listener) {
      const observers = this.observers;
      if (!observers || observers[observers.length - 1] !== Listener) {
        const sSlot = observers ? observers.length : 0;
        if (!Listener.sources) {
          Listener.sources = [this];
          Listener.sourceSlots = [sSlot];
        } else {
          Listener.sources.push(this);
          Listener.sourceSlots.push(sSlot);
        }
        if (!observers) {
          this.observers = [Listener];
          this.observerSlots = [Listener.sources.length - 1];
        } else {
          observers.push(Listener);
          this.observerSlots.push(Listener.sources.length - 1);
        }
      }
    }
    if (runningTransition && Transition.sources.has(this))
      return this.tValue;
    return this.value;
  }
  function writeSignal(node, value, isComp) {
    let current = Transition && Transition.running && Transition.sources.has(node) ? node.tValue : node.value;
    if (!node.comparator || !node.comparator(current, value)) {
      if (Transition) {
        const TransitionRunning = Transition.running;
        if (TransitionRunning || !isComp && Transition.sources.has(node)) {
          Transition.sources.add(node);
          node.tValue = value;
        }
        if (!TransitionRunning)
          node.value = value;
      } else
        node.value = value;
      if (node.observers && node.observers.length) {
        runUpdates(() => {
          for (let i = 0;i < node.observers.length; i += 1) {
            const o = node.observers[i];
            const TransitionRunning = Transition && Transition.running;
            if (TransitionRunning && Transition.disposed.has(o))
              continue;
            if (TransitionRunning ? !o.tState : !o.state) {
              if (o.pure)
                Updates.push(o);
              else
                Effects.push(o);
              if (o.observers)
                markDownstream(o);
            }
            if (!TransitionRunning)
              o.state = STALE;
            else
              o.tState = STALE;
          }
          if (Updates.length > 1e6) {
            Updates = [];
            if (IS_DEV)
              ;
            throw new Error;
          }
        }, false);
      }
    }
    return value;
  }
  function updateComputation(node) {
    if (!node.fn)
      return;
    cleanNode(node);
    const time = ExecCount;
    runComputation(node, Transition && Transition.running && Transition.sources.has(node) ? node.tValue : node.value, time);
    if (Transition && !Transition.running && Transition.sources.has(node)) {
      queueMicrotask(() => {
        runUpdates(() => {
          Transition && (Transition.running = true);
          Listener = Owner = node;
          runComputation(node, node.tValue, time);
          Listener = Owner = null;
        }, false);
      });
    }
  }
  function runComputation(node, value, time) {
    let nextValue;
    const owner = Owner, listener = Listener;
    Listener = Owner = node;
    try {
      nextValue = node.fn(value);
    } catch (err) {
      if (node.pure) {
        if (Transition && Transition.running) {
          node.tState = STALE;
          node.tOwned && node.tOwned.forEach(cleanNode);
          node.tOwned = undefined;
        } else {
          node.state = STALE;
          node.owned && node.owned.forEach(cleanNode);
          node.owned = null;
        }
      }
      node.updatedAt = time + 1;
      return handleError(err);
    } finally {
      Listener = listener;
      Owner = owner;
    }
    if (!node.updatedAt || node.updatedAt <= time) {
      if (node.updatedAt != null && "observers" in node) {
        writeSignal(node, nextValue, true);
      } else if (Transition && Transition.running && node.pure) {
        if (!Transition.sources.has(node))
          node.value = nextValue;
        Transition.sources.add(node);
        node.tValue = nextValue;
      } else
        node.value = nextValue;
      node.updatedAt = time;
    }
  }
  function createComputation(fn, init, pure, state = STALE, options) {
    const c = {
      fn,
      state,
      updatedAt: null,
      owned: null,
      sources: null,
      sourceSlots: null,
      cleanups: null,
      value: init,
      owner: Owner,
      context: Owner ? Owner.context : null,
      pure
    };
    if (Transition && Transition.running) {
      c.state = 0;
      c.tState = state;
    }
    if (Owner === null)
      ;
    else if (Owner !== UNOWNED) {
      if (Transition && Transition.running && Owner.pure) {
        if (!Owner.tOwned)
          Owner.tOwned = [c];
        else
          Owner.tOwned.push(c);
      } else {
        if (!Owner.owned)
          Owner.owned = [c];
        else
          Owner.owned.push(c);
      }
    }
    if (ExternalSourceConfig && c.fn) {
      const sourceFn = c.fn;
      const [track, trigger] = createSignal(undefined, {
        equals: false
      });
      const ordinary = ExternalSourceConfig.factory(sourceFn, trigger);
      onCleanup(() => ordinary.dispose());
      let inTransition;
      const triggerInTransition = () => startTransition(trigger).then(() => {
        if (inTransition) {
          inTransition.dispose();
          inTransition = undefined;
        }
      });
      c.fn = (x) => {
        track();
        if (Transition && Transition.running) {
          if (!inTransition)
            inTransition = ExternalSourceConfig.factory(sourceFn, triggerInTransition);
          return inTransition.track(x);
        }
        return ordinary.track(x);
      };
    }
    return c;
  }
  function runTop(node) {
    const runningTransition = Transition && Transition.running;
    if ((runningTransition ? node.tState : node.state) === 0)
      return;
    if ((runningTransition ? node.tState : node.state) === PENDING)
      return lookUpstream(node);
    if (node.suspense && untrack(node.suspense.inFallback))
      return node.suspense.effects.push(node);
    const ancestors = [node];
    while ((node = node.owner) && (!node.updatedAt || node.updatedAt < ExecCount)) {
      if (runningTransition && Transition.disposed.has(node))
        return;
      if (runningTransition ? node.tState : node.state)
        ancestors.push(node);
    }
    for (let i = ancestors.length - 1;i >= 0; i--) {
      node = ancestors[i];
      if (runningTransition) {
        let top = node, prev = ancestors[i + 1];
        while ((top = top.owner) && top !== prev) {
          if (Transition.disposed.has(top))
            return;
        }
      }
      if ((runningTransition ? node.tState : node.state) === STALE) {
        updateComputation(node);
      } else if ((runningTransition ? node.tState : node.state) === PENDING) {
        const updates = Updates;
        Updates = null;
        runUpdates(() => lookUpstream(node, ancestors[0]), false);
        Updates = updates;
      }
    }
  }
  function runUpdates(fn, init) {
    if (Updates)
      return fn();
    let wait = false;
    if (!init)
      Updates = [];
    if (Effects)
      wait = true;
    else
      Effects = [];
    ExecCount++;
    try {
      const res = fn();
      completeUpdates(wait);
      return res;
    } catch (err) {
      if (!wait)
        Effects = null;
      Updates = null;
      handleError(err);
    }
  }
  function completeUpdates(wait) {
    if (Updates) {
      if (Scheduler && Transition && Transition.running)
        scheduleQueue(Updates);
      else
        runQueue(Updates);
      Updates = null;
    }
    if (wait)
      return;
    let res;
    if (Transition) {
      if (!Transition.promises.size && !Transition.queue.size) {
        const sources = Transition.sources;
        const disposed = Transition.disposed;
        Effects.push.apply(Effects, Transition.effects);
        res = Transition.resolve;
        for (const e2 of Effects) {
          "tState" in e2 && (e2.state = e2.tState);
          delete e2.tState;
        }
        Transition = null;
        runUpdates(() => {
          for (const d of disposed)
            cleanNode(d);
          for (const v of sources) {
            v.value = v.tValue;
            if (v.owned) {
              for (let i = 0, len = v.owned.length;i < len; i++)
                cleanNode(v.owned[i]);
            }
            if (v.tOwned)
              v.owned = v.tOwned;
            delete v.tValue;
            delete v.tOwned;
            v.tState = 0;
          }
          setTransPending(false);
        }, false);
      } else if (Transition.running) {
        Transition.running = false;
        Transition.effects.push.apply(Transition.effects, Effects);
        Effects = null;
        setTransPending(true);
        return;
      }
    }
    const e = Effects;
    Effects = null;
    if (e.length)
      runUpdates(() => runEffects(e), false);
    if (res)
      res();
  }
  function runQueue(queue) {
    for (let i = 0;i < queue.length; i++)
      runTop(queue[i]);
  }
  function scheduleQueue(queue) {
    for (let i = 0;i < queue.length; i++) {
      const item = queue[i];
      const tasks = Transition.queue;
      if (!tasks.has(item)) {
        tasks.add(item);
        Scheduler(() => {
          tasks.delete(item);
          runUpdates(() => {
            Transition.running = true;
            runTop(item);
          }, false);
          Transition && (Transition.running = false);
        });
      }
    }
  }
  function lookUpstream(node, ignore) {
    const runningTransition = Transition && Transition.running;
    if (runningTransition)
      node.tState = 0;
    else
      node.state = 0;
    for (let i = 0;i < node.sources.length; i += 1) {
      const source = node.sources[i];
      if (source.sources) {
        const state = runningTransition ? source.tState : source.state;
        if (state === STALE) {
          if (source !== ignore && (!source.updatedAt || source.updatedAt < ExecCount))
            runTop(source);
        } else if (state === PENDING)
          lookUpstream(source, ignore);
      }
    }
  }
  function markDownstream(node) {
    const runningTransition = Transition && Transition.running;
    for (let i = 0;i < node.observers.length; i += 1) {
      const o = node.observers[i];
      if (runningTransition ? !o.tState : !o.state) {
        if (runningTransition)
          o.tState = PENDING;
        else
          o.state = PENDING;
        if (o.pure)
          Updates.push(o);
        else
          Effects.push(o);
        o.observers && markDownstream(o);
      }
    }
  }
  function cleanNode(node) {
    let i;
    if (node.sources) {
      while (node.sources.length) {
        const source = node.sources.pop(), index = node.sourceSlots.pop(), obs = source.observers;
        if (obs && obs.length) {
          const n = obs.pop(), s = source.observerSlots.pop();
          if (index < obs.length) {
            n.sourceSlots[s] = index;
            obs[index] = n;
            source.observerSlots[index] = s;
          }
        }
      }
    }
    if (node.tOwned) {
      for (i = node.tOwned.length - 1;i >= 0; i--)
        cleanNode(node.tOwned[i]);
      delete node.tOwned;
    }
    if (Transition && Transition.running && node.pure) {
      reset(node, true);
    } else if (node.owned) {
      for (i = node.owned.length - 1;i >= 0; i--)
        cleanNode(node.owned[i]);
      node.owned = null;
    }
    if (node.cleanups) {
      for (i = node.cleanups.length - 1;i >= 0; i--)
        node.cleanups[i]();
      node.cleanups = null;
    }
    if (Transition && Transition.running)
      node.tState = 0;
    else
      node.state = 0;
  }
  function reset(node, top) {
    if (!top) {
      node.tState = 0;
      Transition.disposed.add(node);
    }
    if (node.owned) {
      for (let i = 0;i < node.owned.length; i++)
        reset(node.owned[i]);
    }
  }
  function castError(err) {
    if (err instanceof Error)
      return err;
    return new Error(typeof err === "string" ? err : "Unknown error", {
      cause: err
    });
  }
  function runErrors(err, fns, owner) {
    try {
      for (const f of fns)
        f(err);
    } catch (e) {
      handleError(e, owner && owner.owner || null);
    }
  }
  function handleError(err, owner = Owner) {
    const fns = ERROR && owner && owner.context && owner.context[ERROR];
    const error = castError(err);
    if (!fns)
      throw error;
    if (Effects)
      Effects.push({
        fn() {
          runErrors(error, fns, owner);
        },
        state: STALE
      });
    else
      runErrors(error, fns, owner);
  }
  var FALLBACK = Symbol("fallback");
  function dispose(d) {
    for (let i = 0;i < d.length; i++)
      d[i]();
  }
  function mapArray(list, mapFn, options = {}) {
    let items = [], mapped = [], disposers = [], len = 0, indexes = mapFn.length > 1 ? [] : null;
    onCleanup(() => dispose(disposers));
    return () => {
      let newItems = list() || [], newLen = newItems.length, i, j;
      newItems[$TRACK];
      return untrack(() => {
        let newIndices, newIndicesNext, temp, tempdisposers, tempIndexes, start, end, newEnd, item;
        if (newLen === 0) {
          if (len !== 0) {
            dispose(disposers);
            disposers = [];
            items = [];
            mapped = [];
            len = 0;
            indexes && (indexes = []);
          }
          if (options.fallback) {
            items = [FALLBACK];
            mapped[0] = createRoot((disposer) => {
              disposers[0] = disposer;
              return options.fallback();
            });
            len = 1;
          }
        } else if (len === 0) {
          mapped = new Array(newLen);
          for (j = 0;j < newLen; j++) {
            items[j] = newItems[j];
            mapped[j] = createRoot(mapper);
          }
          len = newLen;
        } else {
          temp = new Array(newLen);
          tempdisposers = new Array(newLen);
          indexes && (tempIndexes = new Array(newLen));
          for (start = 0, end = Math.min(len, newLen);start < end && items[start] === newItems[start]; start++)
            ;
          for (end = len - 1, newEnd = newLen - 1;end >= start && newEnd >= start && items[end] === newItems[newEnd]; end--, newEnd--) {
            temp[newEnd] = mapped[end];
            tempdisposers[newEnd] = disposers[end];
            indexes && (tempIndexes[newEnd] = indexes[end]);
          }
          newIndices = new Map;
          newIndicesNext = new Array(newEnd + 1);
          for (j = newEnd;j >= start; j--) {
            item = newItems[j];
            i = newIndices.get(item);
            newIndicesNext[j] = i === undefined ? -1 : i;
            newIndices.set(item, j);
          }
          for (i = start;i <= end; i++) {
            item = items[i];
            j = newIndices.get(item);
            if (j !== undefined && j !== -1) {
              temp[j] = mapped[i];
              tempdisposers[j] = disposers[i];
              indexes && (tempIndexes[j] = indexes[i]);
              j = newIndicesNext[j];
              newIndices.set(item, j);
            } else
              disposers[i]();
          }
          for (j = start;j < newLen; j++) {
            if (j in temp) {
              mapped[j] = temp[j];
              disposers[j] = tempdisposers[j];
              if (indexes) {
                indexes[j] = tempIndexes[j];
                indexes[j](j);
              }
            } else
              mapped[j] = createRoot(mapper);
          }
          mapped = mapped.slice(0, len = newLen);
          items = newItems.slice(0);
        }
        return mapped;
      });
      function mapper(disposer) {
        disposers[j] = disposer;
        if (indexes) {
          const [s, set] = createSignal(j);
          indexes[j] = set;
          return mapFn(newItems[j], s);
        }
        return mapFn(newItems[j]);
      }
    };
  }
  var hydrationEnabled = false;
  function createComponent(Comp, props) {
    if (hydrationEnabled) {
      if (sharedConfig.context) {
        const c = sharedConfig.context;
        setHydrateContext(nextHydrateContext());
        const r = untrack(() => Comp(props || {}));
        setHydrateContext(c);
        return r;
      }
    }
    return untrack(() => Comp(props || {}));
  }
  function trueFn() {
    return true;
  }
  var propTraps = {
    get(_, property, receiver) {
      if (property === $PROXY)
        return receiver;
      return _.get(property);
    },
    has(_, property) {
      if (property === $PROXY)
        return true;
      return _.has(property);
    },
    set: trueFn,
    deleteProperty: trueFn,
    getOwnPropertyDescriptor(_, property) {
      return {
        configurable: true,
        enumerable: true,
        get() {
          return _.get(property);
        },
        set: trueFn,
        deleteProperty: trueFn
      };
    },
    ownKeys(_) {
      return _.keys();
    }
  };
  function resolveSource(s) {
    return !(s = typeof s === "function" ? s() : s) ? {} : s;
  }
  function resolveSources() {
    for (let i = 0, length = this.length;i < length; ++i) {
      const v = this[i]();
      if (v !== undefined)
        return v;
    }
  }
  function mergeProps(...sources) {
    let proxy = false;
    for (let i = 0;i < sources.length; i++) {
      const s = sources[i];
      proxy = proxy || !!s && $PROXY in s;
      sources[i] = typeof s === "function" ? (proxy = true, createMemo(s)) : s;
    }
    if (SUPPORTS_PROXY && proxy) {
      return new Proxy({
        get(property) {
          for (let i = sources.length - 1;i >= 0; i--) {
            const v = resolveSource(sources[i])[property];
            if (v !== undefined)
              return v;
          }
        },
        has(property) {
          for (let i = sources.length - 1;i >= 0; i--) {
            if (property in resolveSource(sources[i]))
              return true;
          }
          return false;
        },
        keys() {
          const keys = [];
          for (let i = 0;i < sources.length; i++)
            keys.push(...Object.keys(resolveSource(sources[i])));
          return [...new Set(keys)];
        }
      }, propTraps);
    }
    const sourcesMap = {};
    const defined = Object.create(null);
    for (let i = sources.length - 1;i >= 0; i--) {
      const source = sources[i];
      if (!source)
        continue;
      const sourceKeys = Object.getOwnPropertyNames(source);
      for (let i2 = sourceKeys.length - 1;i2 >= 0; i2--) {
        const key = sourceKeys[i2];
        if (key === "__proto__" || key === "constructor")
          continue;
        const desc = Object.getOwnPropertyDescriptor(source, key);
        if (!defined[key]) {
          defined[key] = desc.get ? {
            enumerable: true,
            configurable: true,
            get: resolveSources.bind(sourcesMap[key] = [desc.get.bind(source)])
          } : desc.value !== undefined ? desc : undefined;
        } else {
          const sources2 = sourcesMap[key];
          if (sources2) {
            if (desc.get)
              sources2.push(desc.get.bind(source));
            else if (desc.value !== undefined)
              sources2.push(() => desc.value);
          }
        }
      }
    }
    const target = {};
    const definedKeys = Object.keys(defined);
    for (let i = definedKeys.length - 1;i >= 0; i--) {
      const key = definedKeys[i], desc = defined[key];
      if (desc && desc.get)
        Object.defineProperty(target, key, desc);
      else
        target[key] = desc ? desc.value : undefined;
    }
    return target;
  }
  var narrowedError = (name) => `Stale read from <${name}>.`;
  function For(props) {
    const fallback = "fallback" in props && {
      fallback: () => props.fallback
    };
    return createMemo(mapArray(() => props.each, props.children, fallback || undefined));
  }
  function Show(props) {
    const keyed = props.keyed;
    const conditionValue = createMemo(() => props.when, undefined, undefined);
    const condition = keyed ? conditionValue : createMemo(conditionValue, undefined, {
      equals: (a, b) => !a === !b
    });
    return createMemo(() => {
      const c = condition();
      if (c) {
        const child = props.children;
        const fn = typeof child === "function" && child.length > 0;
        return fn ? untrack(() => child(keyed ? c : () => {
          if (!untrack(condition))
            throw narrowedError("Show");
          return conditionValue();
        })) : child;
      }
      return props.fallback;
    }, undefined, undefined);
  }

  // vendor/pocketjs/node_modules/solid-js/universal/dist/universal.js
  var memo = (fn) => createMemo(() => fn());
  function createRenderer$1({
    createElement,
    createTextNode,
    isTextNode,
    replaceText,
    insertNode,
    removeNode,
    setProperty,
    getParentNode,
    getFirstChild,
    getNextSibling
  }) {
    function insert(parent, accessor, marker, initial) {
      if (marker !== undefined && !initial)
        initial = [];
      if (typeof accessor !== "function")
        return insertExpression(parent, accessor, initial, marker);
      createRenderEffect((current) => insertExpression(parent, accessor(), current, marker), initial);
    }
    function insertExpression(parent, value, current, marker, unwrapArray) {
      while (typeof current === "function")
        current = current();
      if (value === current)
        return current;
      const t = typeof value, multi = marker !== undefined;
      if (t === "string" || t === "number") {
        if (t === "number")
          value = value.toString();
        if (multi) {
          let node = current[0];
          if (node && isTextNode(node)) {
            replaceText(node, value);
          } else
            node = createTextNode(value);
          current = cleanChildren(parent, current, marker, node);
        } else {
          if (current !== "" && typeof current === "string") {
            replaceText(getFirstChild(parent), current = value);
          } else {
            cleanChildren(parent, current, marker, createTextNode(value));
            current = value;
          }
        }
      } else if (value == null || t === "boolean") {
        current = cleanChildren(parent, current, marker);
      } else if (t === "function") {
        createRenderEffect(() => {
          let v = value();
          while (typeof v === "function")
            v = v();
          current = insertExpression(parent, v, current, marker);
        });
        return () => current;
      } else if (Array.isArray(value)) {
        const array = [];
        if (normalizeIncomingArray(array, value, unwrapArray)) {
          createRenderEffect(() => current = insertExpression(parent, array, current, marker, true));
          return () => current;
        }
        if (array.length === 0) {
          const replacement = cleanChildren(parent, current, marker);
          if (multi)
            return current = replacement;
        } else {
          if (Array.isArray(current)) {
            if (current.length === 0) {
              appendNodes(parent, array, marker);
            } else
              reconcileArrays(parent, current, array);
          } else if (current == null || current === "") {
            appendNodes(parent, array);
          } else {
            reconcileArrays(parent, multi && current || [getFirstChild(parent)], array);
          }
        }
        current = array;
      } else {
        if (Array.isArray(current)) {
          if (multi)
            return current = cleanChildren(parent, current, marker, value);
          cleanChildren(parent, current, null, value);
        } else if (current == null || current === "" || !getFirstChild(parent)) {
          insertNode(parent, value);
        } else
          replaceNode(parent, value, getFirstChild(parent));
        current = value;
      }
      return current;
    }
    function normalizeIncomingArray(normalized, array, unwrap) {
      let dynamic = false;
      for (let i = 0, len = array.length;i < len; i++) {
        let item = array[i], t;
        if (item == null || item === true || item === false)
          ;
        else if (Array.isArray(item)) {
          dynamic = normalizeIncomingArray(normalized, item) || dynamic;
        } else if ((t = typeof item) === "string" || t === "number") {
          normalized.push(createTextNode(item));
        } else if (t === "function") {
          if (unwrap) {
            while (typeof item === "function")
              item = item();
            dynamic = normalizeIncomingArray(normalized, Array.isArray(item) ? item : [item]) || dynamic;
          } else {
            normalized.push(item);
            dynamic = true;
          }
        } else
          normalized.push(item);
      }
      return dynamic;
    }
    function reconcileArrays(parentNode, a, b) {
      let bLength = b.length, aEnd = a.length, bEnd = bLength, aStart = 0, bStart = 0, after = getNextSibling(a[aEnd - 1]), map = null;
      while (aStart < aEnd || bStart < bEnd) {
        if (a[aStart] === b[bStart]) {
          aStart++;
          bStart++;
          continue;
        }
        while (a[aEnd - 1] === b[bEnd - 1]) {
          aEnd--;
          bEnd--;
        }
        if (aEnd === aStart) {
          const node = bEnd < bLength ? bStart ? getNextSibling(b[bStart - 1]) : b[bEnd - bStart] : after;
          while (bStart < bEnd)
            insertNode(parentNode, b[bStart++], node);
        } else if (bEnd === bStart) {
          while (aStart < aEnd) {
            if (!map || !map.has(a[aStart]))
              removeNode(parentNode, a[aStart]);
            aStart++;
          }
        } else if (a[aStart] === b[bEnd - 1] && b[bStart] === a[aEnd - 1]) {
          const node = getNextSibling(a[--aEnd]);
          insertNode(parentNode, b[bStart++], getNextSibling(a[aStart++]));
          insertNode(parentNode, b[--bEnd], node);
          a[aEnd] = b[bEnd];
        } else {
          if (!map) {
            map = new Map;
            let i = bStart;
            while (i < bEnd)
              map.set(b[i], i++);
          }
          const index = map.get(a[aStart]);
          if (index != null) {
            if (bStart < index && index < bEnd) {
              let i = aStart, sequence = 1, t;
              while (++i < aEnd && i < bEnd) {
                if ((t = map.get(a[i])) == null || t !== index + sequence)
                  break;
                sequence++;
              }
              if (sequence > index - bStart) {
                const node = a[aStart];
                while (bStart < index)
                  insertNode(parentNode, b[bStart++], node);
              } else
                replaceNode(parentNode, b[bStart++], a[aStart++]);
            } else
              aStart++;
          } else
            removeNode(parentNode, a[aStart++]);
        }
      }
    }
    function cleanChildren(parent, current, marker, replacement) {
      if (marker === undefined) {
        let removed;
        while (removed = getFirstChild(parent))
          removeNode(parent, removed);
        replacement && insertNode(parent, replacement);
        return "";
      }
      const node = replacement || createTextNode("");
      if (current.length) {
        let inserted = false;
        for (let i = current.length - 1;i >= 0; i--) {
          const el = current[i];
          if (node !== el) {
            const isParent = getParentNode(el) === parent;
            if (!inserted && !i)
              isParent ? replaceNode(parent, node, el) : insertNode(parent, node, marker);
            else
              isParent && removeNode(parent, el);
          } else
            inserted = true;
        }
      } else
        insertNode(parent, node, marker);
      return [node];
    }
    function appendNodes(parent, array, marker) {
      for (let i = 0, len = array.length;i < len; i++)
        insertNode(parent, array[i], marker);
    }
    function replaceNode(parent, newNode, oldNode) {
      insertNode(parent, newNode, oldNode);
      removeNode(parent, oldNode);
    }
    function spreadExpression(node, props, prevProps = {}, skipChildren) {
      props || (props = {});
      if (!skipChildren) {
        createRenderEffect(() => prevProps.children = insertExpression(node, props.children, prevProps.children));
      }
      createRenderEffect(() => props.ref && props.ref(node));
      createRenderEffect(() => {
        for (const prop in props) {
          if (prop === "children" || prop === "ref")
            continue;
          const value = props[prop];
          if (value === prevProps[prop])
            continue;
          setProperty(node, prop, value, prevProps[prop]);
          prevProps[prop] = value;
        }
      });
      return prevProps;
    }
    return {
      render(code, element) {
        let disposer;
        createRoot((dispose2) => {
          disposer = dispose2;
          insert(element, code());
        });
        return disposer;
      },
      insert,
      spread(node, accessor, skipChildren) {
        if (typeof accessor === "function") {
          createRenderEffect((current) => spreadExpression(node, accessor(), current, skipChildren));
        } else
          spreadExpression(node, accessor, undefined, skipChildren);
      },
      createElement,
      createTextNode,
      insertNode,
      setProp(node, name, value, prev) {
        setProperty(node, name, value, prev);
        return value;
      },
      mergeProps,
      effect: createRenderEffect,
      memo,
      createComponent,
      use(fn, element, arg) {
        return untrack(() => fn(element, arg));
      }
    };
  }
  function createRenderer(options) {
    const renderer = createRenderer$1(options);
    renderer.mergeProps = mergeProps;
    return renderer;
  }

  // vendor/pocketjs/spec/spec.ts
  var SCREEN_W = 480;
  var SCREEN_H = 272;
  var NODE_TYPE = {
    view: 0,
    text: 1,
    image: 2
  };
  var ROOT_ID = 1;
  var STYLE_ID_NONE = -1;
  var PROP = {
    width: 1,
    height: 2,
    minW: 3,
    minH: 4,
    maxW: 5,
    maxH: 6,
    paddingT: 8,
    paddingR: 9,
    paddingB: 10,
    paddingL: 11,
    marginT: 12,
    marginR: 13,
    marginB: 14,
    marginL: 15,
    gap: 16,
    flexDir: 17,
    justify: 18,
    align: 19,
    grow: 20,
    shrink: 21,
    basis: 22,
    flexWrap: 23,
    posType: 24,
    insetT: 25,
    insetR: 26,
    insetB: 27,
    insetL: 28,
    display: 29,
    overflow: 30,
    zIndex: 31,
    bgColor: 64,
    gradFrom: 65,
    gradTo: 66,
    gradDir: 67,
    radius: 68,
    opacity: 69,
    borderColor: 70,
    borderWidth: 71,
    shadow: 72,
    textColor: 96,
    fontSlot: 97,
    textAlign: 98,
    lineHeight: 99,
    tracking: 100,
    translateX: 128,
    translateY: 129,
    scale: 130,
    rotate: 131,
    scaleX: 132,
    scaleY: 133,
    originX: 134,
    originY: 135,
    rotateX: 136,
    rotateY: 137,
    translateZ: 138,
    perspective: 139,
    arcStart: 140,
    arcSweep: 141,
    arcWidth: 142
  };
  var VALUE_KIND = {
    f32: 0,
    color: 1,
    int: 2
  };
  var PROP_VALUE_KIND = {
    width: VALUE_KIND.f32,
    height: VALUE_KIND.f32,
    minW: VALUE_KIND.f32,
    minH: VALUE_KIND.f32,
    maxW: VALUE_KIND.f32,
    maxH: VALUE_KIND.f32,
    paddingT: VALUE_KIND.f32,
    paddingR: VALUE_KIND.f32,
    paddingB: VALUE_KIND.f32,
    paddingL: VALUE_KIND.f32,
    marginT: VALUE_KIND.f32,
    marginR: VALUE_KIND.f32,
    marginB: VALUE_KIND.f32,
    marginL: VALUE_KIND.f32,
    gap: VALUE_KIND.f32,
    flexDir: VALUE_KIND.int,
    justify: VALUE_KIND.int,
    align: VALUE_KIND.int,
    grow: VALUE_KIND.f32,
    shrink: VALUE_KIND.f32,
    basis: VALUE_KIND.f32,
    flexWrap: VALUE_KIND.int,
    posType: VALUE_KIND.int,
    insetT: VALUE_KIND.f32,
    insetR: VALUE_KIND.f32,
    insetB: VALUE_KIND.f32,
    insetL: VALUE_KIND.f32,
    display: VALUE_KIND.int,
    overflow: VALUE_KIND.int,
    zIndex: VALUE_KIND.int,
    bgColor: VALUE_KIND.color,
    gradFrom: VALUE_KIND.color,
    gradTo: VALUE_KIND.color,
    gradDir: VALUE_KIND.int,
    radius: VALUE_KIND.f32,
    opacity: VALUE_KIND.f32,
    borderColor: VALUE_KIND.color,
    borderWidth: VALUE_KIND.f32,
    shadow: VALUE_KIND.int,
    textColor: VALUE_KIND.color,
    fontSlot: VALUE_KIND.int,
    textAlign: VALUE_KIND.int,
    lineHeight: VALUE_KIND.f32,
    tracking: VALUE_KIND.f32,
    translateX: VALUE_KIND.f32,
    translateY: VALUE_KIND.f32,
    scale: VALUE_KIND.f32,
    rotate: VALUE_KIND.f32,
    scaleX: VALUE_KIND.f32,
    scaleY: VALUE_KIND.f32,
    originX: VALUE_KIND.f32,
    originY: VALUE_KIND.f32,
    rotateX: VALUE_KIND.f32,
    rotateY: VALUE_KIND.f32,
    translateZ: VALUE_KIND.f32,
    perspective: VALUE_KIND.f32,
    arcStart: VALUE_KIND.f32,
    arcSweep: VALUE_KIND.f32,
    arcWidth: VALUE_KIND.f32
  };
  var ENUMS = {
    FlexDir: {
      Row: 0,
      Col: 1
    },
    Justify: {
      Start: 0,
      Center: 1,
      End: 2,
      Between: 3,
      Around: 4
    },
    Align: {
      Start: 0,
      Center: 1,
      End: 2,
      Stretch: 3
    },
    PosType: {
      Relative: 0,
      Absolute: 1
    },
    Display: {
      Flex: 0,
      None: 1
    },
    Overflow: {
      Visible: 0,
      Hidden: 1
    },
    TextAlign: {
      Left: 0,
      Center: 1,
      Right: 2
    },
    GradDir: {
      ToTop: 0,
      ToBottom: 1,
      ToLeft: 2,
      ToRight: 3
    },
    Easing: {
      Linear: 0,
      EaseIn: 1,
      EaseOut: 2,
      EaseInOut: 3,
      OutBack: 4,
      Spring: 5,
      SpringBouncy: 6,
      CubicBezier: 7
    }
  };
  var STYLE_VARIANT_BASE = 1 << 0;
  var STYLE_VARIANT_FOCUS = 1 << 1;
  var STYLE_VARIANT_ACTIVE = 1 << 2;
  var STYLE_HAS_TRANSITION = 1 << 3;
  var STYLE_HAS_ANIMATION = 1 << 4;
  var ANIM_FILL_BACKWARDS = 1 << 0;
  var ANIM_FILL_FORWARDS = 1 << 1;
  function abgr(r, g, b, a = 255) {
    return ((a & 255) << 24 | (b & 255) << 16 | (g & 255) << 8 | r & 255) >>> 0;
  }
  var FONT_FLAG_BOLD = 1 << 0;
  var PAK_MAGIC = 1263551300;
  var PAK_VERSION = 1;
  var PAK_HEADER_SIZE = 32;
  var PAK_ENTRY_SIZE = 24;
  var BTN = {
    SELECT: 1,
    START: 8,
    UP: 16,
    RIGHT: 32,
    DOWN: 64,
    LEFT: 128,
    LTRIGGER: 256,
    RTRIGGER: 512,
    TRIANGLE: 4096,
    CIRCLE: 8192,
    CROSS: 16384,
    SQUARE: 32768
  };
  var FIXED_DT = 1 / 60;

  // vendor/pocketjs/src/host.ts
  var current = null;
  function detectHost(injected) {
    const native = globalThis.ui;
    if (injected) {
      if (native !== undefined && injected === native && native.__textures !== undefined) {
        return {
          ops: injected,
          kind: "psp",
          strict: false
        };
      }
      return {
        ops: injected,
        kind: "injected",
        strict: true
      };
    }
    if (native)
      return {
        ops: native,
        kind: "psp",
        strict: false
      };
    throw new Error("PocketJS: no host — pass HostOps to render() (web/test) or run under the PSP runtime (globalThis.ui)");
  }
  function installHost(host) {
    current = host;
  }
  function getHost() {
    if (!current) {
      throw new Error("PocketJS: host not installed — call render() first");
    }
    return current;
  }
  function getOps() {
    return getHost().ops;
  }
  function installFrameHandler(fn) {
    globalThis.frame = fn;
  }
  function parseHexColor(s) {
    let hex = s.slice(1);
    if (hex.length === 3) {
      hex = hex[0] + hex[0] + hex[1] + hex[1] + hex[2] + hex[2];
    }
    if (hex.length !== 6 && hex.length !== 8) {
      throw new Error(`PocketJS: bad color '${s}' (expected #rgb/#rrggbb/#rrggbbaa)`);
    }
    if (!/^[0-9a-fA-F]+$/.test(hex))
      throw new Error(`PocketJS: bad color '${s}'`);
    const n = parseInt(hex, 16);
    if (hex.length === 6) {
      return abgr(n >>> 16 & 255, n >>> 8 & 255, n & 255, 255);
    }
    return abgr(n >>> 24 & 255, n >>> 16 & 255, n >>> 8 & 255, n & 255);
  }
  function encodePropValue(prop, value) {
    const kind = PROP_VALUE_KIND[prop];
    if (typeof value === "string") {
      if (kind === VALUE_KIND.color)
        return parseHexColor(value);
      const n = Number(value);
      if (Number.isNaN(n)) {
        throw new Error(`PocketJS: non-numeric value '${value}' for prop '${prop}'`);
      }
      value = n;
    }
    if (kind === VALUE_KIND.color || kind === VALUE_KIND.int)
      return value >>> 0;
    return value;
  }

  // vendor/pocketjs/src/input.ts
  var root = null;
  var focused = null;
  var prevButtons = 0;
  var focusScopeStack = [];
  var focusGridStack = [];
  function setInputRoot(r) {
    root = r;
    focused = null;
    prevButtons = 0;
    focusScopeStack.length = 0;
    focusGridStack.length = 0;
  }
  function registerPress(node, fn) {
    node.onPress = fn ?? undefined;
  }
  function registerFocusable(node, on) {
    node.focusable = on;
    if (!on && focused === node) {
      focusNode(null);
    }
  }
  function focusNode(node) {
    focused = node;
    getOps().setFocus(node ? node.id : 0);
  }
  function activeFocusRoot() {
    return focusScopeStack.length > 0 ? focusScopeStack[focusScopeStack.length - 1] : root;
  }
  function collectFocusables(node, out) {
    if (!node)
      return;
    if (node.focusable)
      out.push(node);
    if (!Array.isArray(node.children))
      return;
    for (let i = 0;i < node.children.length; i++) {
      collectFocusables(node.children[i], out);
    }
  }
  function focusables() {
    const out = [];
    const r = activeFocusRoot();
    if (r)
      collectFocusables(r, out);
    return out;
  }
  function linearDirection(direction) {
    return direction === "down" || direction === "right" ? 1 : -1;
  }
  function moveLinearFocus(direction) {
    const dir = linearDirection(direction);
    const list = focusables();
    if (list.length === 0) {
      if (focused)
        focusNode(null);
      return;
    }
    const i = focused ? list.indexOf(focused) : -1;
    if (i < 0) {
      focusNode(dir === 1 ? list[0] : list[list.length - 1]);
      return;
    }
    const j = i + dir;
    if (j < 0 || j >= list.length)
      return;
    focusNode(list[j]);
  }
  function activeGrid() {
    if (!focused)
      return null;
    const active = activeFocusRoot();
    if (active && !isWithin(focused, active))
      return null;
    for (let i = focusGridStack.length - 1;i >= 0; i--) {
      const grid = focusGridStack[i];
      if (active && !isWithin(grid.node, active) && !isWithin(active, grid.node))
        continue;
      if (isWithin(focused, grid.node))
        return grid;
    }
    return null;
  }
  function moveGridFocus(direction) {
    const grid = activeGrid();
    if (!grid)
      return false;
    const list = [];
    collectFocusables(grid.node, list);
    if (list.length === 0) {
      if (focused)
        focusNode(null);
      return true;
    }
    const columns = grid.columns;
    const i = focused ? list.indexOf(focused) : -1;
    if (i < 0) {
      focusNode(linearDirection(direction) === 1 ? list[0] : list[list.length - 1]);
      return true;
    }
    let j = i;
    switch (direction) {
      case "right":
        if (i + 1 < list.length && i % columns < columns - 1)
          j = i + 1;
        else if (grid.wrap)
          j = Math.floor(i / columns) * columns;
        break;
      case "left":
        if (i % columns > 0)
          j = i - 1;
        else if (grid.wrap)
          j = Math.min(list.length - 1, Math.floor(i / columns) * columns + columns - 1);
        break;
      case "down":
        if (i + columns < list.length)
          j = i + columns;
        else if (grid.wrap)
          j = i % columns;
        break;
      case "up":
        if (i - columns >= 0)
          j = i - columns;
        else if (grid.wrap) {
          const col = i % columns;
          j = col;
          while (j + columns < list.length)
            j += columns;
        }
        break;
    }
    if (j !== i)
      focusNode(list[j]);
    return true;
  }
  function moveFocus(direction) {
    if (moveGridFocus(direction))
      return;
    moveLinearFocus(direction);
  }
  function firePress() {
    let n = focused;
    while (n) {
      if (n.onPress) {
        n.onPress();
        return;
      }
      n = n.parent;
    }
  }
  function isWithin(node, ancestor) {
    if (!node || !ancestor)
      return false;
    let n = node;
    while (n) {
      if (n === ancestor)
        return true;
      n = n.parent;
    }
    return false;
  }
  function firstFocusable(node) {
    if (!node)
      return null;
    if (node.focusable)
      return node;
    if (!Array.isArray(node.children))
      return null;
    for (let i = 0;i < node.children.length; i++) {
      const f = firstFocusable(node.children[i]);
      if (f)
        return f;
    }
    return null;
  }
  function notifyDetached(node) {
    if (!focused || !isWithin(focused, node))
      return;
    const parent = node.parent;
    if (parent) {
      const idx = parent.children.indexOf(node);
      for (let i = idx + 1;i < parent.children.length; i++) {
        const f = firstFocusable(parent.children[i]);
        if (f) {
          focusNode(f);
          return;
        }
      }
      for (let i = idx - 1;i >= 0; i--) {
        const f = firstFocusable(parent.children[i]);
        if (f) {
          focusNode(f);
          return;
        }
      }
      let a = parent;
      while (a) {
        if (a.focusable) {
          focusNode(a);
          return;
        }
        a = a.parent;
      }
    }
    focusNode(null);
  }
  function handleFrame(buttons) {
    const pressed = buttons & ~prevButtons;
    prevButtons = buttons;
    if (pressed === 0)
      return;
    if (pressed & BTN.DOWN)
      moveFocus("down");
    if (pressed & BTN.RIGHT)
      moveFocus("right");
    if (pressed & BTN.UP)
      moveFocus("up");
    if (pressed & BTN.LEFT)
      moveFocus("left");
    if (pressed & BTN.CIRCLE)
      firePress();
  }

  // vendor/pocketjs/src/native-tree.ts
  var treeMutationHook = null;
  function setTreeMutationHook(fn) {
    treeMutationHook = fn;
  }
  function treeMutated() {
    if (treeMutationHook)
      treeMutationHook();
  }
  function setDebugName(node, name) {
    node.debugName = name || undefined;
    treeMutated();
  }
  var rootMirror = {
    id: ROOT_ID,
    type: NODE_TYPE.view,
    parent: null,
    children: [],
    domNodeType: 1,
    domTag: "root"
  };
  var DOM_NODE = Symbol.for("pocketjs.native-node");
  var DOM_ELEMENT = 1;
  var DOM_TEXT = 3;
  var DOM_COMMENT = 8;
  var NATIVE_ATTRIBUTE_NAMES = new Set(["class", "className", "style", "src", "onPress", "on:press", "focusable", "debugName", "ref", "nodeRef", "key", "children"]);
  function domAttrs(node) {
    return node.domAttrs ??= {};
  }
  function cloneNativeNode(node, deep) {
    const nodeType = node.domNodeType ?? (isTextNode(node) ? DOM_TEXT : DOM_ELEMENT);
    const clone = nodeType === DOM_TEXT ? createTextNode(node.text ?? "") : nodeType === DOM_COMMENT ? createCommentNode(node.domData ?? "") : createElement(node.domTag ?? tagName(node));
    for (const key of Object.keys(node.domAttrs ?? {})) {
      setDomAttribute(clone, key, node.domAttrs[key]);
    }
    if (deep) {
      for (const child of node.children)
        insertNode(clone, cloneNativeNode(child, true));
    }
    return clone;
  }
  function setDomAttribute(node, name, value) {
    if (NATIVE_ATTRIBUTE_NAMES.has(name)) {
      setProp(node, name, value, node.domAttrs?.[name]);
      return;
    }
    if (value == null)
      delete domAttrs(node)[name];
    else
      domAttrs(node)[name] = value;
  }
  function decorateNativeNode(node) {
    if (node[DOM_NODE] === true)
      return node;
    Object.defineProperty(node, DOM_NODE, {
      value: true
    });
    Object.defineProperties(node, {
      nodeType: {
        configurable: true,
        get() {
          return node.domNodeType ?? (isTextNode(node) ? DOM_TEXT : DOM_ELEMENT);
        }
      },
      nodeValue: {
        configurable: true,
        get() {
          return node.domNodeType === DOM_COMMENT ? node.domData ?? "" : node.text ?? "";
        },
        set(value) {
          if (node.domNodeType === DOM_COMMENT)
            node.domData = String(value ?? "");
          else
            replaceText(node, String(value ?? ""));
        }
      },
      data: {
        configurable: true,
        get() {
          return node.domNodeType === DOM_COMMENT ? node.domData ?? "" : node.text ?? "";
        },
        set(value) {
          if (node.domNodeType === DOM_COMMENT)
            node.domData = String(value ?? "");
          else
            replaceText(node, String(value ?? ""));
        }
      },
      textContent: {
        configurable: true,
        get() {
          if (node.domNodeType === DOM_COMMENT)
            return node.domData ?? "";
          if (isTextNode(node))
            return node.text ?? "";
          return node.children.map((child) => child.text ?? "").join("");
        },
        set(value) {
          const text = String(value ?? "");
          if (node.domNodeType === DOM_COMMENT) {
            node.domData = text;
          } else if (isTextNode(node)) {
            replaceText(node, text);
          } else {
            clearContainer(node);
            if (text)
              insertNode(node, createTextNode(text));
          }
        }
      },
      parentNode: {
        configurable: true,
        get() {
          return node.parent;
        }
      },
      parentElement: {
        configurable: true,
        get() {
          return node.parent;
        }
      },
      childNodes: {
        configurable: true,
        get() {
          return node.children;
        }
      },
      firstChild: {
        configurable: true,
        get() {
          return node.children[0] ?? null;
        }
      },
      lastChild: {
        configurable: true,
        get() {
          return node.children[node.children.length - 1] ?? null;
        }
      },
      nextSibling: {
        configurable: true,
        get() {
          return getNextSibling(node) ?? null;
        }
      },
      previousSibling: {
        configurable: true,
        get() {
          const parent = node.parent;
          if (!parent)
            return null;
          const index = parent.children.indexOf(node);
          return index > 0 ? parent.children[index - 1] : null;
        }
      },
      tagName: {
        configurable: true,
        get() {
          return (node.domTag ?? tagName(node)).toUpperCase();
        }
      },
      nodeName: {
        configurable: true,
        get() {
          if (node.domNodeType === DOM_TEXT)
            return "#text";
          if (node.domNodeType === DOM_COMMENT)
            return "#comment";
          return (node.domTag ?? tagName(node)).toUpperCase();
        }
      },
      className: {
        configurable: true,
        get() {
          return String(node.domAttrs?.class ?? "");
        },
        set(value) {
          setProp(node, "class", value, node.domAttrs?.class);
        }
      },
      isConnected: {
        configurable: true,
        get() {
          let current2 = node;
          while (current2) {
            if (current2 === rootMirror)
              return true;
            current2 = current2.parent;
          }
          return false;
        }
      }
    });
    const methods = {
      appendChild(child) {
        insertNode(node, child);
        return child;
      },
      insertBefore(child, anchor) {
        insertNode(node, child, anchor ?? null);
        return child;
      },
      removeChild(child) {
        removeNode(node, child);
        return child;
      },
      replaceChild(next, current2) {
        insertNode(node, next, current2);
        removeNode(node, current2);
        return current2;
      },
      cloneNode(deep = false) {
        return cloneNativeNode(node, !!deep);
      },
      remove() {
        if (node.parent)
          removeNode(node.parent, node);
      },
      setAttribute(name, value) {
        setDomAttribute(node, name, value);
      },
      removeAttribute(name) {
        setDomAttribute(node, name, undefined);
      },
      getAttribute(name) {
        const value = node.domAttrs?.[name];
        return value == null ? null : String(value);
      },
      hasAttribute(name) {
        return node.domAttrs?.[name] != null;
      },
      hasChildNodes() {
        return node.children.length > 0;
      },
      contains(other) {
        let current2 = other ?? null;
        while (current2) {
          if (current2 === node)
            return true;
          current2 = current2.parent;
        }
        return false;
      },
      addEventListener() {},
      removeEventListener() {}
    };
    Object.assign(node, methods, {
      style: {
        length: 0,
        item: () => ""
      },
      classList: {
        add() {},
        remove() {}
      }
    });
    return node;
  }
  decorateNativeNode(rootMirror);
  var styleResolver = null;
  function setStyleResolver(fn) {
    styleResolver = fn;
  }
  var missCounters = {
    unknownClass: 0,
    unknownTexture: 0
  };
  var textures = new Map;
  function registerTexture(key, handle) {
    textures.set(key, handle);
  }
  var sprites = new Map;
  function registerSprite(key, meta) {
    sprites.set(key, meta);
  }
  var sweepSet = new Set;
  var retained = new Set;
  function subtreeHasRetained(node) {
    if (!node)
      return false;
    if (retained.has(node))
      return true;
    for (let i = 0;i < node.children.length; i++) {
      if (subtreeHasRetained(node.children[i]))
        return true;
    }
    return false;
  }
  function runSweep() {
    if (sweepSet.size === 0)
      return;
    const ops = getOps();
    const keep = [];
    for (const node of sweepSet) {
      if (!node)
        continue;
      if (node.parent !== null)
        continue;
      if (subtreeHasRetained(node)) {
        keep.push(node);
        continue;
      }
      ops.destroyNode(node.id);
    }
    sweepSet.clear();
    for (let i = 0;i < keep.length; i++)
      sweepSet.add(keep[i]);
  }
  function createElement(tag) {
    const type = NODE_TYPE[tag];
    if (type === undefined) {
      throw new Error(`PocketJS: unknown element <${tag}> - only view/text/image exist`);
    }
    return decorateNativeNode({
      id: getOps().createNode(type),
      type,
      parent: null,
      children: [],
      domNodeType: DOM_ELEMENT,
      domTag: tag
    });
  }
  function createTextNode(value) {
    const ops = getOps();
    const id = ops.createNode(NODE_TYPE.text);
    ops.setText(id, value);
    return decorateNativeNode({
      id,
      type: NODE_TYPE.text,
      parent: null,
      children: [],
      text: value,
      domNodeType: DOM_TEXT,
      domTag: "#text"
    });
  }
  function createCommentNode(data = "") {
    const node = createTextNode("");
    node.domNodeType = DOM_COMMENT;
    node.domTag = "#comment";
    node.domData = data;
    return node;
  }
  function replaceText(node, value) {
    getOps().replaceText(node.id, value);
    node.text = value;
    treeMutated();
  }
  function isTextNode(node) {
    return node.type === NODE_TYPE.text;
  }
  function unlink(node) {
    const p = node.parent;
    if (!p)
      return;
    const i = p.children.indexOf(node);
    if (i >= 0)
      p.children.splice(i, 1);
    node.parent = null;
  }
  function insertNode(parent, node, anchor) {
    const ops = getOps();
    unlink(node);
    sweepSet.delete(node);
    ops.insertBefore(parent.id, node.id, anchor ? anchor.id : 0);
    if (anchor) {
      const i = parent.children.indexOf(anchor);
      if (i < 0)
        throw new Error("PocketJS: insert anchor is not a child of parent");
      parent.children.splice(i, 0, node);
    } else {
      parent.children.push(node);
    }
    node.parent = parent;
    treeMutated();
  }
  function removeNode(parent, node) {
    if (!node)
      return;
    notifyDetached(node);
    getOps().removeChild(parent.id, node.id);
    unlink(node);
    sweepSet.add(node);
    treeMutated();
  }
  function getParentNode(node) {
    return node.parent ?? undefined;
  }
  function getFirstChild(node) {
    return node.children[0];
  }
  function getNextSibling(node) {
    const p = node.parent;
    if (!p)
      return;
    const i = p.children.indexOf(node);
    return i >= 0 ? p.children[i + 1] : undefined;
  }
  function setClass(node, value) {
    const ops = getOps();
    treeMutated();
    if (value == null || value === "") {
      ops.setStyle(node.id, STYLE_ID_NONE);
      return;
    }
    if (typeof value !== "string") {
      throw new Error("PocketJS: class must be a string literal of utilities");
    }
    const styleId = styleResolver ? styleResolver(value) : undefined;
    if (styleId === undefined) {
      if (getHost().strict) {
        throw new Error(`PocketJS: unknown class "${value}" - not in the compiled style table ` + "(dynamic classes must be ternaries of full literals)");
      }
      missCounters.unknownClass++;
      return;
    }
    ops.setStyle(node.id, styleId);
  }
  function setSrc(node, value) {
    const ops = getOps();
    if (value == null || value === "") {
      ops.setImage(node.id, -1);
      return;
    }
    if (typeof value !== "string") {
      throw new Error("PocketJS: src must be a string key");
    }
    const handle = textures.get(value);
    if (handle === undefined) {
      if (getHost().strict) {
        throw new Error(`PocketJS: unknown image src "${value}" - no texture registered under that key`);
      }
      missCounters.unknownTexture++;
      return;
    }
    ops.setImage(node.id, handle);
  }
  function setSpriteSrc(node, value) {
    const ops = getOps();
    if (value == null || value === "") {
      ops.setSprite(node.id, -1, 0, 0, 0);
      return;
    }
    if (typeof value !== "string") {
      throw new Error("PocketJS: sprite must be a string key");
    }
    const meta = sprites.get(value);
    if (meta === undefined) {
      if (getHost().strict) {
        throw new Error(`PocketJS: unknown sprite "${value}" - no sprite atlas registered under that key`);
      }
      missCounters.unknownTexture++;
      return;
    }
    ops.setSprite(node.id, meta.handle, meta.frames, meta.cols, meta.step);
  }
  function setStyleObject(node, value, prev) {
    const ops = getOps();
    const next = value ?? {};
    const before = prev ?? {};
    for (const key in next) {
      const v = next[key];
      if (before[key] === v)
        continue;
      const propId = PROP[key];
      if (propId === undefined) {
        throw new Error(`PocketJS: unknown style prop '${key}' (see spec PROP)`);
      }
      ops.setProp(node.id, propId, encodePropValue(key, v));
    }
  }
  function setProp(node, name, value, prev) {
    if (value === prev && name !== "style")
      return value;
    if (name === "className")
      name = "class";
    if (name !== "children" && name !== "key" && name !== "ref" && name !== "nodeRef") {
      if (value == null)
        delete domAttrs(node)[name];
      else
        domAttrs(node)[name] = value;
    }
    switch (name) {
      case "class":
        setClass(node, value);
        return value;
      case "onPress":
      case "on:press":
        registerPress(node, value);
        return value;
      case "src":
        setSrc(node, value);
        return value;
      case "sprite":
        setSpriteSrc(node, value);
        return value;
      case "style":
        setStyleObject(node, value, prev);
        return value;
      case "focusable":
        registerFocusable(node, !!value);
        return value;
      case "debugName":
        setDebugName(node, value == null ? undefined : String(value));
        return value;
      case "ref":
      case "nodeRef":
      case "key":
      case "children":
        return value;
      default:
        break;
    }
    if (name === "classList") {
      throw new Error("PocketJS: classList is not supported - use ternaries of full class literals");
    }
    if (name.startsWith("on:") || name.startsWith("bool:") || name.startsWith("prop:")) {
      throw new Error(`PocketJS: unsupported namespaced attribute '${name}'`);
    }
    throw new Error(`PocketJS: unknown property '${name}' on <${tagName(node)}>`);
  }
  function clearContainer(container) {
    for (const child of [...container.children])
      removeNode(container, child);
  }
  function tagName(node) {
    for (const key of Object.keys(NODE_TYPE)) {
      if (NODE_TYPE[key] === node.type)
        return key;
    }
    return String(node.type);
  }

  // vendor/pocketjs/src/renderer-solid.ts
  function setProperty(node, name, value, prev) {
    if (name === "ref" && typeof value === "function") {
      value(node);
      return;
    }
    setProp(node, name, value, prev);
  }
  var renderer = createRenderer({
    createElement,
    createTextNode,
    replaceText,
    isTextNode,
    setProperty,
    insertNode(parent, node, anchor) {
      insertNode(parent, node, anchor);
    },
    removeNode(parent, node) {
      removeNode(parent, node);
    },
    getParentNode,
    getFirstChild,
    getNextSibling
  });
  var {
    render,
    effect,
    memo: memo2,
    createComponent: createComponent2,
    createElement: createElement2,
    insert,
    spread,
    mergeProps: mergeProps2,
    use
  } = renderer;

  // game/sdk.ts
  var native = globalThis.strike;
  if (!native) {
    throw new Error("openstrike: no `strike` surface — is this running under the game host?");
  }
  var current2 = {
    time: 0,
    phase: "starting",
    hp: 100,
    alive: true,
    ammo: 30,
    reserve: 90,
    reloading: false,
    reloadFrac: 0,
    aliveBots: 0,
    totalBots: 0,
    wins: 0,
    losses: 0,
    speed: 0
  };
  var handlers = new Map;
  var tickHandlers = new Set;
  native.__dispatch = (state, events) => {
    current2 = state;
    for (const e of events) {
      const set = handlers.get(e.type);
      if (set)
        for (const h of [...set])
          h(e);
    }
    for (const h of [...tickHandlers])
      h(state);
  };
  var strike = {
    state: () => current2,
    on(type, fn) {
      let set = handlers.get(type);
      if (!set)
        handlers.set(type, set = new Set);
      set.add(fn);
      return () => set.delete(fn);
    },
    onTick(fn) {
      tickHandlers.add(fn);
      return () => tickHandlers.delete(fn);
    },
    setPhase: (phase) => native.setPhase(phase),
    resetRound: () => native.resetRound(),
    addWin: () => native.addWin(),
    addLoss: () => native.addLoss(),
    setBotCount: (n) => native.setBotCount(n),
    configureWeapon: (cfg) => native.configureWeapon(cfg),
    configureBots: (cfg) => native.configureBots(cfg)
  };

  // game/rules.ts
  var ROUND_FREEZE = 1.2;
  var ROUND_END_PAUSE = 3.5;
  strike.configureWeapon({
    magSize: 30,
    reserve: 90,
    fireInterval: 0.105,
    reloadTime: 2.4,
    damageBody: 34,
    damageHead: 100
  });
  strike.configureBots({
    count: 3,
    speed: 190,
    attackInterval: 1.4,
    damageMin: 8,
    damageMax: 14
  });
  var [phaseAge, setPhaseAge] = createSignal(0);
  var lastPhase = "";
  var phaseStart = 0;
  strike.onTick((s) => {
    if (s.phase !== lastPhase) {
      lastPhase = s.phase;
      phaseStart = s.time;
    }
    setPhaseAge(s.time - phaseStart);
    if (s.phase === "starting" && s.time - phaseStart >= ROUND_FREEZE) {
      strike.setPhase("live");
    }
    if (s.phase === "live" && s.totalBots > 0 && s.aliveBots === 0) {
      strike.addWin();
      strike.setPhase("won");
    }
    if ((s.phase === "won" || s.phase === "lost") && s.time - phaseStart >= ROUND_END_PAUSE) {
      strike.resetRound();
    }
  });
  strike.on("playerDied", () => {
    strike.addLoss();
    strike.setPhase("lost");
  });

  // vendor/pocketjs/src/devtools.ts
  var TAPE_CAP = 36000;
  var TREE_THROTTLE = 30;
  var STATS_EVERY = 30;
  var state = {
    ops: null,
    transport: null,
    app: undefined,
    frame: 0,
    tape: new Uint16Array(TAPE_CAP),
    tapeStart: 0,
    tapeLen: 0,
    tapeFirstFrame: 0,
    replayMasks: null,
    replayAt: 0,
    paused: false,
    stepQueued: 0,
    inspectReportId: null,
    inspectAskedAt: 0,
    treeDirty: true,
    treeSentAt: -TREE_THROTTLE,
    saidHello: false,
    hostCalls: 0
  };
  function initDevtools(ops) {
    const g = globalThis;
    if (!g.console)
      g.console = {
        log() {},
        warn() {},
        error() {}
      };
    state.ops = ops;
    state.frame = 0;
    state.tapeStart = 0;
    state.tapeLen = 0;
    state.tapeFirstFrame = 0;
    state.replayMasks = null;
    state.paused = false;
    state.stepQueued = 0;
    state.inspectReportId = null;
    state.inspectAskedAt = 0;
    state.treeDirty = true;
    state.treeSentAt = -TREE_THROTTLE;
    state.saidHello = false;
    state.hostCalls = 0;
    state.app = globalThis.__pocketApp;
    const injected = globalThis.__pocketDevtoolsTransport;
    if (injected) {
      state.transport = injected;
    } else if (ops.__dbgActive?.() && ops.__dbgPoll && ops.__dbgSend) {
      state.transport = {
        send: (l) => ops.__dbgSend(l),
        recv: () => ops.__dbgPoll(),
        everyFrames: 10
      };
    } else {
      state.transport = null;
    }
    if (state.transport) {
      setTreeMutationHook(() => {
        state.treeDirty = true;
      });
      bridgeConsole();
    } else {
      setTreeMutationHook(null);
    }
    globalThis.__pocketDevtools = api;
  }
  function wrapFrameHandler(h) {
    return (buttons) => {
      state.hostCalls++;
      if (state.transport) {
        pollTransport();
        flushInspectReport();
      }
      let mask = buttons;
      if (state.replayMasks) {
        if (state.replayAt < state.replayMasks.length) {
          mask = state.replayMasks[state.replayAt++];
        } else {
          state.replayMasks = null;
          send({
            t: "replayDone",
            frame: state.frame
          });
        }
      }
      if (state.paused) {
        if (state.stepQueued <= 0)
          return;
        state.stepQueued--;
        state.ops?.debugStep?.();
      }
      recordMask(mask);
      state.frame++;
      try {
        h(mask);
      } catch (e) {
        send({
          t: "error",
          frame: state.frame,
          message: e instanceof Error ? e.message : String(e),
          stack: e instanceof Error ? e.stack : undefined
        });
        throw e;
      }
      if (state.transport)
        afterFrame();
    };
  }
  function recordMask(mask) {
    if (state.tapeLen < TAPE_CAP) {
      state.tape[(state.tapeStart + state.tapeLen) % TAPE_CAP] = mask;
      state.tapeLen++;
    } else {
      state.tape[state.tapeStart] = mask;
      state.tapeStart = (state.tapeStart + 1) % TAPE_CAP;
      state.tapeFirstFrame++;
    }
  }
  function exportTape() {
    const masks = [];
    for (let i = 0;i < state.tapeLen; i++) {
      const m = state.tape[(state.tapeStart + i) % TAPE_CAP];
      const last = masks[masks.length - 1];
      if (last && last[0] === m)
        last[1]++;
      else
        masks.push([m, 1]);
    }
    return {
      v: 1,
      app: state.app,
      frames: state.tapeLen,
      masks,
      startFrame: state.tapeFirstFrame
    };
  }
  function expandTape(tape) {
    let total = 0;
    for (const [, n] of tape.masks)
      total += n;
    const out = new Uint16Array(total);
    let at = 0;
    for (const [mask, n] of tape.masks) {
      out.fill(mask, at, at + n);
      at += n;
    }
    return out;
  }
  function send(msg) {
    try {
      state.transport?.send(JSON.stringify(msg));
    } catch {}
  }
  function pollTransport() {
    const t = state.transport;
    const every = t.everyFrames ?? 1;
    if (every > 1 && state.hostCalls % every !== 0)
      return;
    if (!state.saidHello) {
      state.saidHello = true;
      send({
        t: "hello",
        app: state.app,
        host: hostKind(),
        frame: state.frame
      });
    }
    for (let guard = 0;guard < 64; guard++) {
      const chunk = t.recv();
      if (!chunk)
        break;
      for (const line of chunk.split(`
`)) {
        if (line.trim())
          handleMessage(line);
      }
    }
  }
  function handleMessage(line) {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    const ops = state.ops;
    switch (msg.t) {
      case "inspect": {
        const id = typeof msg.id === "number" ? msg.id : 0;
        ops?.debugInspect?.(id);
        state.inspectReportId = id || null;
        state.inspectAskedAt = state.hostCalls;
        if (!id)
          send({
            t: "inspect",
            id: 0,
            rect: null
          });
        break;
      }
      case "pause":
        state.paused = true;
        state.stepQueued = 0;
        ops?.debugPause?.(true);
        sendStats();
        break;
      case "resume":
        state.paused = false;
        ops?.debugPause?.(false);
        sendStats();
        break;
      case "step":
        state.stepQueued += typeof msg.n === "number" && msg.n > 0 ? msg.n : 1;
        break;
      case "getTree":
        sendTree();
        break;
      case "eval": {
        let ok = true;
        let value;
        try {
          value = fmt((0, eval)(String(msg.code)));
        } catch (e) {
          ok = false;
          value = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
        }
        send({
          t: "evalResult",
          id: msg.id,
          ok,
          value
        });
        break;
      }
      case "dumpTape":
        send({
          t: "tape",
          tape: exportTape()
        });
        break;
      case "screenshot": {
        if (ops?.__dbgShot?.()) {
          send({
            t: "screenshotRaw",
            file: "shot.raw",
            w: 480,
            h: 272,
            stride: 512,
            frame: state.frame
          });
        } else {
          send({
            t: "log",
            level: "warn",
            args: ["screenshot: not supported on this host"]
          });
        }
        break;
      }
      case "replay": {
        const tape = msg.tape;
        if (tape && Array.isArray(tape.masks)) {
          state.replayMasks = expandTape(tape);
          state.replayAt = 0;
        }
        break;
      }
      default:
        break;
    }
  }
  function afterFrame() {
    if (state.treeDirty && state.frame - state.treeSentAt >= TREE_THROTTLE) {
      sendTree();
    }
    if (state.frame % STATS_EVERY === 0)
      sendStats();
  }
  function flushInspectReport() {
    const id = state.inspectReportId;
    if (id == null)
      return;
    const ops = state.ops;
    if (!ops?.debugRectXY || !ops.debugRectWH) {
      state.inspectReportId = null;
      return;
    }
    const xy = ops.debugRectXY();
    if (xy === -1) {
      if (state.hostCalls - state.inspectAskedAt > 60) {
        state.inspectReportId = null;
        send({
          t: "inspect",
          id,
          rect: null
        });
      }
      return;
    }
    const wh = ops.debugRectWH();
    state.inspectReportId = null;
    send({
      t: "inspect",
      id,
      rect: [xy << 16 >> 16, xy >> 16, wh & 65535, wh >> 16 & 65535]
    });
  }
  function sendStats() {
    send({
      t: "stats",
      frame: state.frame,
      nodes: countNodes(rootMirror),
      tapeLen: state.tapeLen,
      paused: state.paused
    });
  }
  function sendTree() {
    state.treeDirty = false;
    state.treeSentAt = state.frame;
    send({
      t: "tree",
      frame: state.frame,
      root: serializeNode(rootMirror)
    });
  }
  function serializeNode(node) {
    const out = {
      i: node.id,
      t: node.domTag ?? String(node.type)
    };
    if (node.debugName)
      out.n = node.debugName;
    const cls = node.domAttrs?.class;
    if (typeof cls === "string" && cls)
      out.c = cls;
    if (node.text)
      out.x = node.text.length > 80 ? node.text.slice(0, 79) + "…" : node.text;
    const kids = [];
    for (const child of node.children) {
      if (child.domNodeType === 8)
        continue;
      kids.push(serializeNode(child));
    }
    if (kids.length)
      out.k = kids;
    return out;
  }
  function countNodes(node) {
    let n = 1;
    for (const child of node.children)
      n += countNodes(child);
    return n;
  }
  function bridgeConsole() {
    const g = globalThis;
    if (!g.console)
      g.console = {};
    const c = g.console;
    if (c.__pocketBridged)
      return;
    c.__pocketBridged = true;
    for (const level of ["log", "warn", "error"]) {
      const original = c[level];
      c[level] = (...args) => {
        send({
          t: "log",
          level,
          args: args.map((a) => fmt(a))
        });
        original?.apply(c, args);
      };
    }
  }
  function fmt(v, depth = 0) {
    if (v === undefined)
      return "undefined";
    if (v === null)
      return "null";
    const t = typeof v;
    if (t === "string") {
      const s = v;
      return depth === 0 ? clip(s) : JSON.stringify(clip(s));
    }
    if (t === "number" || t === "boolean" || t === "bigint")
      return String(v);
    if (t === "function") {
      const name = v.name;
      return name ? `[function ${name}]` : "[function]";
    }
    if (depth >= 3)
      return Array.isArray(v) ? "[…]" : "{…}";
    if (Array.isArray(v)) {
      const items = v.slice(0, 20).map((x) => fmt(x, depth + 1));
      if (v.length > 20)
        items.push(`… ${v.length - 20} more`);
      return `[${items.join(", ")}]`;
    }
    if (v instanceof Error)
      return `${v.name}: ${v.message}`;
    const entries = Object.entries(v).slice(0, 20);
    const body = entries.map(([k, x]) => `${k}: ${fmt(x, depth + 1)}`).join(", ");
    return `{${body}}`;
  }
  function clip(s) {
    return s.length > 200 ? s.slice(0, 199) + "…" : s;
  }
  function hostKind() {
    const ops = state.ops;
    if (ops?.__textures !== undefined)
      return "psp";
    if (typeof globalThis.document !== "undefined")
      return "web";
    return "headless";
  }
  var api = {
    get frame() {
      return state.frame;
    },
    dumpTape: () => exportTape(),
    replay: (tape) => {
      state.replayMasks = expandTape(tape);
      state.replayAt = 0;
    }
  };
  // vendor/pocketjs/src/overlay.ts
  var overlayRoot = null;
  function setOverlayRoot(root2) {
    overlayRoot = root2;
  }

  // vendor/pocketjs/src/styles.ts
  var verbatim = new Map;
  var sortedAlias = new Map;
  function normalize(cls) {
    return cls.trim().replace(/\s+/g, " ");
  }
  function sortTokens(normalized) {
    return normalized.split(" ").sort().join(" ");
  }
  var ALIAS_AMBIGUOUS = -1;
  function registerStyles(table) {
    for (const key of Object.keys(table)) {
      const id = table[key];
      const norm = normalize(key);
      verbatim.set(norm, id);
      const sorted = sortTokens(norm);
      const prev = sortedAlias.get(sorted);
      sortedAlias.set(sorted, prev !== undefined && prev !== id ? ALIAS_AMBIGUOUS : id);
    }
  }
  function resolveStyle(cls) {
    const norm = normalize(cls);
    const hit = verbatim.get(norm);
    if (hit !== undefined)
      return hit;
    const alias = sortedAlias.get(sortTokens(norm));
    return alias === ALIAS_AMBIGUOUS ? undefined : alias;
  }

  // vendor/pocketjs/src/frame.ts
  var callbacks = new Set;
  var buttonHandlerBlockDepth = 0;
  function resetFrameHooks() {
    callbacks.clear();
    buttonHandlerBlockDepth = 0;
  }
  function runFrameHooks(buttons) {
    for (const cb of [...callbacks])
      cb(buttons);
  }
  function onFrame(callback) {
    callbacks.add(callback);
    onCleanup(() => callbacks.delete(callback));
  }

  // vendor/pocketjs/src/pak.ts
  var map = null;
  var bytes = null;
  function readKey(u8, off, len) {
    let s = "";
    for (let i = 0;i < len; i++)
      s += String.fromCharCode(u8[off + i]);
    return s;
  }
  function parse(ab) {
    const dv = new DataView(ab);
    if (ab.byteLength < PAK_HEADER_SIZE || dv.getUint32(0, true) !== PAK_MAGIC) {
      throw new Error("pak: bad magic");
    }
    const version = dv.getUint16(4, true);
    if (version !== PAK_VERSION) {
      throw new Error("pak: unsupported version " + version);
    }
    const entryCount = dv.getUint32(8, true);
    const dirOff = dv.getUint32(12, true);
    const namesOff = dv.getUint32(16, true);
    const u8 = new Uint8Array(ab);
    const m = new Map;
    for (let i = 0;i < entryCount; i++) {
      const e = dirOff + i * PAK_ENTRY_SIZE;
      const blobOff = dv.getUint32(e + 4, true);
      const byteLen = dv.getUint32(e + 8, true);
      const nameOff = dv.getUint32(e + 12, true);
      const nameLen = dv.getUint16(e + 16, true);
      const dtype = u8[e + 18];
      m.set(readKey(u8, namesOff + nameOff, nameLen), {
        off: blobOff,
        len: byteLen,
        dtype
      });
    }
    map = m;
    bytes = u8;
  }
  function loadPack(ab) {
    parse(ab);
  }
  function ensureLoaded() {
    if (map)
      return;
    const ab = globalThis.__pak;
    if (!ab)
      return;
    parse(ab);
  }
  function hasPack() {
    ensureLoaded();
    return map !== null;
  }
  function entries(prefix = "") {
    ensureLoaded();
    if (!map)
      return [];
    const out = [];
    for (const key of map.keys()) {
      if (key.length >= prefix.length && key.slice(0, prefix.length) === prefix) {
        out.push(key);
      }
    }
    out.sort();
    return out;
  }
  function get(key) {
    ensureLoaded();
    const e = map ? map.get(key) : undefined;
    if (!e) {
      throw new Error("pak: missing key " + key + " (no __pak provided, or the pack is incomplete)");
    }
    return bytes.slice(e.off, e.off + e.len);
  }

  // vendor/pocketjs/src/styles.generated.ts
  var STYLE_IDS = {
    grow: 0,
    "w-full h-full": 1,
    "absolute inset-0": 2,
    "absolute inset-0 justify-center items-center": 3,
    "absolute inset-0 flex-col justify-between p-6": 4,
    "flex-row justify-between items-start": 5,
    "flex-col items-center gap-2": 6,
    "text-sm tracking-wide": 7,
    "flex-col items-end gap-2": 8,
    "flex-row items-center gap-3 px-3 py-2": 9,
    "text-sm font-bold tracking-wide": 10,
    "px-3 py-1": 11,
    "flex-row justify-between items-end": 12,
    "flex-col gap-1": 13,
    "flex-row items-end gap-3 px-4 py-2": 14,
    "text-4xl font-bold": 15,
    "flex-col items-end gap-1": 16,
    "flex-row items-end gap-2 px-4 py-2": 17,
    "text-xl font-bold": 18,
    absolute: 19,
    "flex-col items-center gap-2 px-6 py-3": 20,
    "text-2xl font-bold tracking-wide": 21,
    "relative flex-col w-full h-full bg-slate-50 overflow-hidden": 22,
    "absolute inset-0 z-50 flex-col items-center justify-center": 23,
    "absolute inset-0 bg-slate-950": 24,
    "flex-col gap-2 w-[328] p-3 rounded-xl shadow-lg bg-white border-slate-200": 25,
    "absolute left-3 right-3 bottom-3 flex-row items-center justify-between px-2 py-1 rounded-lg shadow-md bg-white border-slate-200": 26,
    "flex-row flex-wrap": 27
  };

  // vendor/pocketjs/src/index.ts
  if (typeof globalThis.queueMicrotask !== "function") {
    globalThis.queueMicrotask = (fn) => {
      Promise.resolve().then(fn);
    };
  }
  var STYLES_KEY = "ui:styles";
  var FONT_PREFIX = "ui:font.";
  var IMG_PREFIX = "ui:img.";
  var SPRITE_PREFIX = "ui:sprite.";
  function globalOps() {
    return globalThis.ui;
  }
  function uploadPakImages(ops) {
    if (ops.__textures)
      return;
    for (const key of entries(IMG_PREFIX)) {
      const blob = get(key);
      const dv = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
      const w = dv.getUint16(0, true);
      const h = dv.getUint16(2, true);
      const psm = blob[4];
      const handle = ops.uploadTexture(blob.subarray(8), w, h, psm);
      if (handle >= 0)
        registerTexture(key.slice(IMG_PREFIX.length), handle);
    }
  }
  function uploadPakSprites(ops) {
    if (ops.__sprites)
      return;
    for (const key of entries(SPRITE_PREFIX)) {
      const blob = get(key);
      const dv = new DataView(blob.buffer, blob.byteOffset, blob.byteLength);
      const w = dv.getUint16(0, true);
      const h = dv.getUint16(2, true);
      const psm = blob[4];
      const frames = dv.getUint16(6, true);
      const cols = dv.getUint16(8, true);
      const step = dv.getUint16(10, true);
      const handle = ops.uploadTexture(blob.subarray(16), w, h, psm);
      if (handle >= 0) {
        registerSprite(key.slice(SPRITE_PREFIX.length), {
          handle,
          frames,
          cols,
          step
        });
      }
    }
  }
  function createLayer(style) {
    const layer = createElement2("view");
    setProp(layer, "style", style, undefined);
    return layer;
  }
  function render2(code, opts = {}) {
    const host = detectHost(opts.ops);
    installHost(host);
    setStyleResolver(resolveStyle);
    if (opts.styles)
      registerStyles(opts.styles);
    if (host.kind === "psp") {
      const tex = host.ops.__textures;
      if (tex) {
        for (const key in tex)
          registerTexture(key, tex[key]);
      }
      const spr = host.ops.__sprites;
      if (spr) {
        for (const key in spr)
          registerSprite(key, spr[key]);
      }
    }
    if (host.kind === "injected") {
      if (opts.pak)
        loadPack(opts.pak);
      if (hasPack()) {
        for (const key of entries()) {
          if (key === STYLES_KEY) {
            host.ops.loadStyles?.(get(key));
          } else if (key.startsWith(FONT_PREFIX)) {
            host.ops.loadFontAtlas?.(get(key));
          }
        }
      }
    }
    const viewport = host.ops.__viewport;
    const layerW = viewport?.w ?? SCREEN_W;
    const layerH = viewport?.h ?? SCREEN_H;
    const appRoot = createLayer({
      width: layerW,
      height: layerH,
      overflow: ENUMS.Overflow.Hidden
    });
    const overlayRoot2 = createLayer({
      width: layerW,
      height: layerH,
      posType: ENUMS.PosType.Absolute,
      insetT: 0,
      insetR: 0,
      insetB: 0,
      insetL: 0,
      zIndex: 1000
    });
    insertNode(rootMirror, appRoot);
    insertNode(rootMirror, overlayRoot2);
    setOverlayRoot(overlayRoot2);
    setInputRoot(appRoot);
    resetFrameHooks();
    initDevtools(host.ops);
    installFrameHandler(wrapFrameHandler((buttons) => {
      runFrameHooks(buttons);
      handleFrame(buttons);
      runSweep();
    }));
    const dispose2 = render(code, appRoot);
    return () => {
      dispose2();
      setInputRoot(null);
      setOverlayRoot(null);
      for (const child of rootMirror.children.splice(0)) {
        child.parent = null;
        host.ops.destroyNode(child.id);
      }
      runSweep();
    };
  }
  function mount(code, opts = {}) {
    const ops = opts.ops ?? globalOps();
    if (!ops) {
      throw new Error("PocketJS: mount() requires globalThis.ui or opts.ops");
    }
    if (opts.pak)
      loadPack(opts.pak);
    uploadPakImages(ops);
    uploadPakSprites(ops);
    const dispose2 = render2(code, {
      ops,
      styles: opts.styles ?? STYLE_IDS,
      pak: opts.pak
    });
    return dispose2;
  }

  // vendor/pocketjs/src/anim.ts
  var EASING_BY_NAME = {
    linear: ENUMS.Easing.Linear,
    in: ENUMS.Easing.EaseIn,
    out: ENUMS.Easing.EaseOut,
    "in-out": ENUMS.Easing.EaseInOut,
    "out-back": ENUMS.Easing.OutBack,
    spring: ENUMS.Easing.Spring,
    "spring-bouncy": ENUMS.Easing.SpringBouncy
  };

  // vendor/pocketjs/src/primitives.ts
  function callRef(ref, node) {
    if (!ref)
      return;
    if (typeof ref === "function")
      ref(node);
    else if ("current" in ref)
      ref.current = node;
  }
  function primitive(tag, props) {
    const el = createElement2(tag);
    spread(el, props, false);
    callRef(props.nodeRef, el);
    return el;
  }
  function View(props) {
    return primitive("view", props);
  }
  function Text(props) {
    return primitive("text", props);
  }
  // game/hud.tsx
  var TICK = 1 / 64;
  var INK = "#e8f0f2";
  var LIME = "#b8f34aee";
  var AMBER = "#fbbf24";
  var RED = "#f43f3f";
  var PANEL = "#080d12aa";
  function Hud() {
    const s0 = strike.state();
    const [phase, setPhase] = createSignal(s0.phase);
    const [hp, setHp] = createSignal(s0.hp);
    const [alive, setAlive] = createSignal(s0.alive);
    const [ammo, setAmmo] = createSignal(s0.ammo);
    const [reserve, setReserve] = createSignal(s0.reserve);
    const [reloading, setReloading] = createSignal(s0.reloading);
    const [reloadFrac, setReloadFrac] = createSignal(s0.reloadFrac);
    const [aliveBots, setAliveBots] = createSignal(s0.aliveBots);
    const [totalBots, setTotalBots] = createSignal(s0.totalBots);
    const [wins, setWins] = createSignal(s0.wins);
    const [losses, setLosses] = createSignal(s0.losses);
    const readState = (s) => {
      setPhase(s.phase);
      setHp(s.hp);
      setAlive(s.alive);
      setAmmo(s.ammo);
      setReserve(s.reserve);
      setReloading(s.reloading);
      setReloadFrac(s.reloadFrac);
      setAliveBots(s.aliveBots);
      setTotalBots(s.totalBots);
      setWins(s.wins);
      setLosses(s.losses);
    };
    const [flash, setFlash] = createSignal(0);
    const [hitmark, setHitmark] = createSignal(0);
    const [feed, setFeed] = createSignal([]);
    let feedId = 0;
    strike.on("playerDamaged", () => setFlash(0.55));
    strike.on("hit", (e) => {
      if (e.type !== "hit")
        return;
      setHitmark(e.headshot ? 0.24 : 0.16);
      if (e.fatal) {
        const text = e.headshot ? "HEADSHOT  ×  HOSTILE DOWN" : "HOSTILE DOWN";
        setFeed((f) => [...f.slice(-3), {
          id: feedId++,
          text,
          ttl: 2.4
        }]);
      }
    });
    strike.on("roundReset", () => {
      setFeed([]);
      setFlash(0);
    });
    onFrame(() => {
      readState(strike.state());
      setFlash((f) => Math.max(0, f - TICK * 1.3));
      setHitmark((h) => Math.max(0, h - TICK));
      setFeed((f) => {
        let dirty = false;
        const next = f.map((e) => ({
          ...e,
          ttl: e.ttl - TICK
        })).filter((e) => e.ttl > 0 ? true : (dirty = true, false));
        return dirty || f.length > 0 ? next : f;
      });
    });
    const hpColor = () => hp() > 60 ? INK : hp() > 25 ? AMBER : RED;
    return createComponent2(View, {
      class: "w-full h-full",
      get children() {
        return [createComponent2(Show, {
          get when() {
            return flash() > 0;
          },
          get children() {
            return createComponent2(View, {
              class: "absolute inset-0",
              get style() {
                return {
                  bgColor: "#c40e0e",
                  opacity: flash() * 0.5,
                  zIndex: 5
                };
              }
            });
          }
        }), createComponent2(Show, {
          get when() {
            return !alive();
          },
          get children() {
            return createComponent2(View, {
              class: "absolute inset-0",
              style: {
                bgColor: "#2a040466",
                zIndex: 5
              }
            });
          }
        }), createComponent2(Show, {
          get when() {
            return alive();
          },
          get children() {
            return createComponent2(View, {
              class: "absolute inset-0 justify-center items-center",
              style: {
                zIndex: 10
              },
              get children() {
                return createComponent2(View, {
                  style: {
                    width: 44,
                    height: 44
                  },
                  get children() {
                    return [createComponent2(Cross, {
                      color: LIME,
                      gap: 16,
                      len: 12,
                      thick: 2
                    }), createComponent2(Show, {
                      get when() {
                        return hitmark() > 0;
                      },
                      get children() {
                        return createComponent2(Cross, {
                          color: RED,
                          gap: 7,
                          len: 7,
                          thick: 3,
                          rotated: true
                        });
                      }
                    })];
                  }
                });
              }
            });
          }
        }), createComponent2(View, {
          class: "absolute inset-0 flex-col justify-between p-6",
          style: {
            zIndex: 20
          },
          get children() {
            return [createComponent2(View, {
              class: "flex-row justify-between items-start",
              get children() {
                return [createComponent2(View, {
                  style: {
                    width: 240
                  }
                }), createComponent2(View, {
                  class: "flex-col items-center gap-2",
                  get children() {
                    return [createComponent2(Show, {
                      get when() {
                        return phase() === "starting";
                      },
                      get children() {
                        return createComponent2(Banner, {
                          color: INK,
                          title: "ROUND START",
                          get children() {
                            return createComponent2(Text, {
                              class: "text-sm tracking-wide",
                              style: {
                                textColor: AMBER
                              },
                              get children() {
                                return "GO IN " + Math.max(0, Math.ceil(ROUND_FREEZE - phaseAge())) + " ";
                              }
                            });
                          }
                        });
                      }
                    }), createComponent2(Show, {
                      get when() {
                        return phase() === "won";
                      },
                      get children() {
                        return createComponent2(Banner, {
                          color: LIME,
                          title: "HOSTILES ELIMINATED",
                          get children() {
                            return createComponent2(Text, {
                              class: "text-sm tracking-wide",
                              style: {
                                textColor: INK
                              },
                              get children() {
                                return "NEXT ROUND IN " + Math.max(0, Math.ceil(ROUND_END_PAUSE - phaseAge())) + " ";
                              }
                            });
                          }
                        });
                      }
                    }), createComponent2(Show, {
                      get when() {
                        return phase() === "lost";
                      },
                      get children() {
                        return createComponent2(Banner, {
                          color: RED,
                          title: "YOU DIED",
                          get children() {
                            return createComponent2(Text, {
                              class: "text-sm tracking-wide",
                              style: {
                                textColor: AMBER
                              },
                              get children() {
                                return "NEXT ROUND IN " + Math.max(0, Math.ceil(ROUND_END_PAUSE - phaseAge())) + " ";
                              }
                            });
                          }
                        });
                      }
                    })];
                  }
                }), createComponent2(View, {
                  class: "flex-col items-end gap-2",
                  style: {
                    width: 240
                  },
                  get children() {
                    return [createComponent2(View, {
                      class: "flex-row items-center gap-3 px-3 py-2",
                      style: {
                        bgColor: PANEL
                      },
                      get children() {
                        return [createComponent2(Text, {
                          class: "text-sm font-bold tracking-wide",
                          style: {
                            textColor: LIME
                          },
                          get children() {
                            return "W " + wins();
                          }
                        }), createComponent2(Text, {
                          class: "text-sm font-bold tracking-wide",
                          style: {
                            textColor: RED
                          },
                          get children() {
                            return "L " + losses();
                          }
                        }), createComponent2(View, {
                          style: {
                            width: 2,
                            height: 14,
                            bgColor: "#e8f0f240"
                          }
                        }), createComponent2(Text, {
                          class: "text-sm font-bold tracking-wide",
                          style: {
                            textColor: AMBER
                          },
                          get children() {
                            return "HOSTILES " + aliveBots() + "/" + totalBots();
                          }
                        })];
                      }
                    }), createComponent2(For, {
                      get each() {
                        return feed();
                      },
                      children: (e) => createComponent2(View, {
                        class: "px-3 py-1",
                        get style() {
                          return {
                            bgColor: PANEL,
                            opacity: Math.min(1, e.ttl / 0.4)
                          };
                        },
                        get children() {
                          return createComponent2(Text, {
                            class: "text-sm font-bold tracking-wide",
                            style: {
                              textColor: INK
                            },
                            get children() {
                              return e.text;
                            }
                          });
                        }
                      })
                    })];
                  }
                })];
              }
            }), createComponent2(View, {
              class: "flex-row justify-between items-end",
              get children() {
                return [createComponent2(View, {
                  class: "flex-col gap-1",
                  get children() {
                    return [createComponent2(View, {
                      class: "flex-row items-end gap-3 px-4 py-2",
                      style: {
                        bgColor: PANEL
                      },
                      get children() {
                        return [createComponent2(Text, {
                          class: "text-sm font-bold tracking-wide",
                          style: {
                            textColor: "#8fa3ad"
                          },
                          children: "HP"
                        }), createComponent2(Text, {
                          class: "text-4xl font-bold",
                          get style() {
                            return {
                              textColor: hpColor()
                            };
                          },
                          get children() {
                            return Math.max(0, hp());
                          }
                        })];
                      }
                    }), createComponent2(View, {
                      style: {
                        width: 220,
                        height: 5,
                        bgColor: "#e8f0f21c"
                      },
                      get children() {
                        return createComponent2(View, {
                          get style() {
                            return {
                              width: Math.max(0, hp() / 100 * 220),
                              height: 5,
                              bgColor: hpColor()
                            };
                          }
                        });
                      }
                    })];
                  }
                }), createComponent2(View, {
                  class: "flex-col items-end gap-1",
                  get children() {
                    return [createComponent2(Show, {
                      get when() {
                        return reloading();
                      },
                      get children() {
                        return createComponent2(View, {
                          class: "flex-col items-end gap-1",
                          get children() {
                            return [createComponent2(Text, {
                              class: "text-sm font-bold tracking-wide",
                              style: {
                                textColor: AMBER
                              },
                              children: "RELOADING"
                            }), createComponent2(View, {
                              style: {
                                width: 160,
                                height: 4,
                                bgColor: "#e8f0f21c"
                              },
                              get children() {
                                return createComponent2(View, {
                                  get style() {
                                    return {
                                      width: reloadFrac() * 160,
                                      height: 4,
                                      bgColor: AMBER
                                    };
                                  }
                                });
                              }
                            })];
                          }
                        });
                      }
                    }), createComponent2(View, {
                      class: "flex-row items-end gap-2 px-4 py-2",
                      style: {
                        bgColor: PANEL
                      },
                      get children() {
                        return [createComponent2(Text, {
                          class: "text-4xl font-bold",
                          get style() {
                            return {
                              textColor: ammo() === 0 ? RED : INK
                            };
                          },
                          get children() {
                            return ammo();
                          }
                        }), createComponent2(Text, {
                          class: "text-xl font-bold",
                          style: {
                            textColor: "#8fa3ad"
                          },
                          get children() {
                            return "/ " + reserve();
                          }
                        })];
                      }
                    })];
                  }
                })];
              }
            })];
          }
        })];
      }
    });
  }
  function Cross(props) {
    const c = 22;
    const bar = (x, y, w, h) => createComponent2(View, {
      class: "absolute",
      get style() {
        return {
          insetL: x,
          insetT: y,
          width: w,
          height: h,
          bgColor: props.color
        };
      }
    });
    return createComponent2(View, {
      class: "absolute inset-0",
      get style() {
        return props.rotated ? {
          rotate: 45
        } : undefined;
      },
      get children() {
        return [memo2(() => bar(c - props.thick / 2, c - props.gap / 2 - props.len, props.thick, props.len)), memo2(() => bar(c - props.thick / 2, c + props.gap / 2, props.thick, props.len)), memo2(() => bar(c - props.gap / 2 - props.len, c - props.thick / 2, props.len, props.thick)), memo2(() => bar(c + props.gap / 2, c - props.thick / 2, props.len, props.thick))];
      }
    });
  }
  function Banner(props) {
    return createComponent2(View, {
      class: "flex-col items-center gap-2 px-6 py-3",
      style: {
        bgColor: PANEL
      },
      get children() {
        return [createComponent2(View, {
          get style() {
            return {
              width: 260,
              height: 2,
              bgColor: props.color
            };
          }
        }), createComponent2(Text, {
          class: "text-2xl font-bold tracking-wide",
          get style() {
            return {
              textColor: props.color
            };
          },
          get children() {
            return props.title;
          }
        }), createComponent2(View, {
          get style() {
            return {
              width: 260,
              height: 2,
              bgColor: props.color
            };
          }
        }), memo2(() => props.children)];
      }
    });
  }

  // game/openstrike.tsx
  mount(() => createComponent2(Hud, {}));
})();
 