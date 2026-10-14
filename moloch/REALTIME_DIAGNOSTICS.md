# Realtime failure diagnosis

Previous direct and aggregate subscriptions failed with `CHANNEL_ERROR / unclassified`. That is not evidence of an authorization failure, a capacity limit, or a specific network fault. The installed official Supabase client can wrap underlying errors, so inspecting only the outer message or code loses useful evidence.

The classifier now examines at most12 unique error nodes through cause, error, reason and a bounded aggregate-error list. It stores only an allowlisted error shape, inspected-node count, category and known network error codes. It never writes raw messages, arbitrary codes, URLs, headers or tokens. Tests cover nested DNS errors, cycles, oversized chains and secret-bearing strings. Authentication and capacity categories remain message-based diagnostic inferences, not proof of the server's cause.

This change does not restart a connection, refresh credentials, increase read budgets, create an account client, or submit orders. A later bounded diagnostic attempt must wait for the existing collector to terminate and preserve every funded paper quote and position. A successful subscription alone does not prove fresh data, a usable strategy, or profit.
