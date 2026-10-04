import {config, requireThat} from '../feed/contract.mjs';

// A frozen universe is an allowlist. New markets on the tournament topic do
// not silently add instruments to the collector or any paper experiment.
export function realtimeScopes({tournamentId, markets, channelMode = 'per_market'} = {}) {
  requireThat(['per_market', 'tournament'].includes(channelMode), 'INVALID_CHANNEL_MODE');
  requireThat(Array.isArray(markets) && markets.length > 0 && markets.length <= (channelMode === 'tournament' ? 300 : 4), 'INVALID_MARKET_SCOPE');
  const scopes = markets.map(m => config({tournamentId, marketId: m.marketId, exchangeIds: [m.exchangeId]}));
  requireThat(new Set(scopes.map(s => s.marketId)).size === scopes.length
    && new Set(scopes.flatMap(s => s.exchangeIds)).size === scopes.length, 'DUPLICATE_MARKET_SCOPE');
  return {scopes, channelMode, topic: channelMode === 'tournament' ? `tournament:${scopes[0].tournamentId}:markets` : null};
}

export function realtimeCaptureScope(activation, baseline, universe) {
  requireThat(activation?.mode === 'market_capture_only' && activation.enabled === true, 'INVALID_REALTIME_ACTIVATION');
  requireThat(activation.schemaVersion === 1 && (activation.channelMode === undefined || activation.channelMode === 'per_market')
    || activation.schemaVersion === 2 && activation.channelMode === 'tournament', 'INVALID_REALTIME_ACTIVATION');
  const channelMode = activation.schemaVersion === 2 ? 'tournament' : 'per_market';
  requireThat(universe?.tournamentId === baseline?.tournamentId && Array.isArray(universe.exchanges), 'INVALID_REALTIME_UNIVERSE');
  const markets = channelMode === 'tournament' ? universe.exchanges.map(({marketId, exchangeId}) => ({marketId, exchangeId}))
    : baseline.marketIds.map(marketId => ({marketId, exchangeId: universe.exchanges.find(m => m.marketId === marketId)?.exchangeId}));
  const scope = {tournamentId: baseline.tournamentId, markets, channelMode};
  realtimeScopes(scope); return scope;
}
