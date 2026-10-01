import {setTimeout as timer} from 'node:timers/promises';
import {snapshotPath, requireThat} from './contract.mjs';
export class Stopped extends Error { constructor() { super('STOPPED'); this.name = 'AbortError'; } }
export class RecoveryError extends Error {
  constructor({status = 503, retryAfter = null} = {}) { super('RECOVERY_UNAVAILABLE'); this.status = status; this.retryAfter = retryAfter; }
}
export function checkStop(signal, shouldStop = () => false) { if (signal?.aborted || shouldStop()) throw new Stopped(); }
export async function interruptibleWait(ms, {signal, shouldStop = () => false, sleep = timer} = {}) {
  requireThat(Number.isFinite(ms) && ms >= 0, 'INVALID_DELAY');
  let remaining = ms;
  do { checkStop(signal, shouldStop); const chunk = Math.min(remaining, 50); if (chunk > 0) await sleep(chunk, undefined, {signal}); remaining -= chunk; } while (remaining > 0);
  checkStop(signal, shouldStop);
}
export function retryAfterMs(header, now) {
  if (typeof header !== 'string' || !header.trim()) return 0;
  const text = header.trim();
  const n = /^\d+(?:\.\d+)?$/.test(text) ? Number(text) * 1000 : (/GMT$/i.test(text) ? Date.parse(text) - now : NaN);
  return Number.isFinite(n) ? Math.max(0, n) : 0;
}
export function backoff(attempt, {baseMs = 250, maxMs = 30000, random = Math.random} = {}) {
  requireThat(Number.isInteger(attempt) && attempt >= 0 && Number.isFinite(baseMs) && baseMs > 0 && Number.isFinite(maxMs) && maxMs >= baseMs);
  const jitter = random(); requireThat(Number.isFinite(jitter) && jitter >= 0 && jitter <= 1);
  return Math.min(maxMs, baseMs * 2 ** Math.min(attempt, 30)) * (0.5 + jitter * 0.5);
}
// Inject the SAME service-backed admission callback across all account clients,
// processes and keys. This wrapper is not itself a distributed limiter.
export class AccountReadBudget {
  constructor({accountScope, admit, cooldown}) {
    requireThat(typeof accountScope === 'string' && accountScope.length > 0 && typeof admit === 'function' && typeof cooldown === 'function', 'SHARED_BUDGET_REQUIRED');
    this.accountScope = accountScope; this.admitCallback = admit; this.cooldownCallback = cooldown;
  }
  async admit(path, {signal, shouldStop} = {}) {
    checkStop(signal, shouldStop);
    await this.admitCallback({accountScope: this.accountScope, method: 'GET', path, cost: 1, priority: 'recovery', signal});
    checkStop(signal, shouldStop);
  }
  async cooldown(ms, {signal} = {}) {
    requireThat(Number.isFinite(ms) && ms >= 0, 'INVALID_DELAY');
    await this.cooldownCallback({accountScope: this.accountScope, retryAfterMs: ms, signal});
  }
}
export class DisabledTransport {
  constructor() { this.mode = 'disabled'; }
  async subscribe() { throw new Error('LIVE_TRANSPORT_DISABLED'); }
  async readSnapshot() { throw new Error('LIVE_TRANSPORT_DISABLED'); }
  async close() {}
}
// Only offline adapters can be exercised here. No network library, auth loading,
// token minting or order-write method is supplied. Integration needs review.
export class OfflineRuntime {
  constructor({collector, adapter = new DisabledTransport(), budget, enabled = false, maxAttempts = 5, maxWaitMs = 120000, requestTimeoutMs = 20000, random = Math.random, sleep = timer, shouldStop = () => false}) {
    requireThat(Number.isInteger(maxAttempts) && maxAttempts > 0 && maxAttempts <= 20 && Number.isFinite(maxWaitMs) && maxWaitMs >= 0 && Number.isFinite(requestTimeoutMs) && requestTimeoutMs > 0);
    this.controller = new AbortController(); this.collector = collector; this.adapter = adapter; this.budget = budget; this.enabled = enabled; this.maxAttempts = maxAttempts;
    this.maxWaitMs = maxWaitMs; this.requestTimeoutMs = requestTimeoutMs; this.random = random; this.sleep = sleep; this.shouldStop = shouldStop;
  }
  guard(signal) { checkStop(signal, this.shouldStop); requireThat(this.enabled && this.adapter.mode === 'offline', 'LIVE_TRANSPORT_DISABLED'); }
  // Timeout/abort races prevent an uncooperative adapter from hanging the caller;
  // future real adapters MUST also abort underlying work and release resources.
  async call(fn, signal) {
    const local = new AbortController();
    const combined = signal ? AbortSignal.any([signal, local.signal]) : local.signal;
    const timeout = setTimeout(() => local.abort(), this.requestTimeoutMs);
    const stopPoll = setInterval(() => { if (this.shouldStop()) local.abort(); }, 25);
    let onAbort;
    const aborted = new Promise((_, reject) => { onAbort = () => reject(new Stopped()); combined.addEventListener('abort', onAbort, {once: true}); if (combined.aborted) onAbort(); });
    try { return await Promise.race([Promise.resolve().then(() => { checkStop(combined, this.shouldStop); return fn(combined); }), aborted]); }
    finally { clearTimeout(timeout); clearInterval(stopPoll); combined.removeEventListener('abort', onAbort); }
  }
  async recover({signal} = {}) {
    const combined = signal ? AbortSignal.any([signal, this.controller.signal]) : this.controller.signal;
    try { return await this.recoverInternal({signal: combined}); }
    catch (error) {
      if (combined.aborted || this.shouldStop() || error?.name === 'AbortError') { this.collector.stop(); await this.closeBounded(); }
      throw error;
    }
  }
  async recoverInternal({signal} = {}) {
    signal = signal ? AbortSignal.any([signal, this.controller.signal]) : this.controller.signal;
    this.guard(signal); requireThat(this.budget instanceof AccountReadBudget, 'SHARED_BUDGET_REQUIRED');
    for (let attempt = 0; attempt < this.maxAttempts; attempt++) {
      this.guard(signal); if (this.collector.state === 'disconnected') return false; const ticket = this.collector.beginRecovery(); const path = snapshotPath(this.collector.scope);
      try {
        await this.call(s => this.budget.admit(path, {signal: s, shouldStop: this.shouldStop}), signal);
        this.guard(signal);
        const raw = await this.call(s => this.adapter.readSnapshot({method: 'GET', path, signal: s}), signal);
        this.guard(signal);
        if (this.collector.completeRecovery(ticket, raw)) return true;
      } catch (error) {
        checkStop(signal, this.shouldStop);
        if (this.collector.state === 'disconnected') return false;
        if (error?.name === 'AbortError') { this.collector.invalidate('RECOVERY_FAILED'); return false; }
        const status = Number.isInteger(error?.status) ? error.status : 503;
        this.collector.invalidate('RECOVERY_FAILED');
        if (status >= 400 && status < 500 && status !== 429) return false;
        const delay = Math.max(status === 429 ? 60000 : 0, retryAfterMs(error?.retryAfter, this.collector.now()), backoff(attempt, {random: this.random}));
        // Share Retry-After with the account before retrying; never silently cap
        // a server cooldown and send too early. Excessive wait stops this run.
        await this.call(s => this.budget.cooldown(delay, {signal: s}), signal);
        if (delay > this.maxWaitMs) return false;
        if (attempt + 1 < this.maxAttempts) await interruptibleWait(delay, {signal, shouldStop: this.shouldStop, sleep: this.sleep});
        continue;
      }
      if (attempt + 1 < this.maxAttempts) await interruptibleWait(backoff(attempt, {random: this.random}), {signal, shouldStop: this.shouldStop, sleep: this.sleep});
    }
    this.collector.invalidate('RECOVERY_FAILED'); return false;
  }
  async connect({signal, reason = 'INITIAL'} = {}) {
    signal = signal ? AbortSignal.any([signal, this.controller.signal]) : this.controller.signal;
    this.guard(signal);
    try {
      // Bound reconnect attempts; callback is registered before REST snapshot.
      for (let attempt = 0; attempt < this.maxAttempts; attempt++) {
        this.guard(signal);
        try {
          const nextEpoch = this.collector.epoch + 1; const pending = []; let subscribed = false;
          await this.call(s => this.adapter.subscribe({topic: this.collector.scope.topic, signal: s,
            onEvent: (event, payload) => {
              if (subscribed) this.collector.ingest(event, payload, {epoch: nextEpoch});
              else { requireThat(pending.length < 1000, 'SUBSCRIPTION_BUFFER_FULL'); pending.push([event, payload]); }
            }}), signal);
          this.guard(signal); this.collector.subscribed(reason); subscribed = true;
          for (const [event, payload] of pending) this.collector.ingest(event, payload, {epoch: nextEpoch});
          const recovered = await this.recover({signal});
          if (!recovered) { this.collector.disconnected('RECOVERY_FAILED'); await this.closeBounded(); }
          return recovered;
        } catch (error) {
          checkStop(signal, this.shouldStop);
          this.collector.disconnected('SOCKET_ERROR');
          await this.closeBounded();
          if (attempt + 1 === this.maxAttempts || error?.name === 'AbortError') return false;
          await interruptibleWait(backoff(attempt, {random: this.random}), {signal, shouldStop: this.shouldStop, sleep: this.sleep});
        }
      }
      return false;
    } catch (error) {
      if (signal?.aborted || this.shouldStop() || error?.name === 'AbortError') { this.collector.stop(); await this.closeBounded(); }
      throw error;
    }
  }
  async closeBounded() {
    const local = new AbortController(); let timeout;
    try { await Promise.race([Promise.resolve().then(() => this.adapter.close({signal: local.signal})), new Promise(resolve => { timeout = setTimeout(() => { local.abort(); resolve(); }, this.requestTimeoutMs); })]); }
    catch { /* Never log raw adapter errors or block fail-closed cleanup. */ }
    finally { clearTimeout(timeout); }
  }
  async stop() { this.controller.abort(); this.collector.stop(); await this.closeBounded(); }
}
