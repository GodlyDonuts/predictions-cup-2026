//! Participant-readable ALL receipts. These observations do not authorize trades.
//!
//! A price-constraint violation is not an executable arbitrage. In particular,
//! never interpret `bound` as a portfolio payout floor, or advance as cash.
use crate::{Result, Scope, decimal_units, fail, need, numeric, parse_clock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub fn uuid(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType { Monotonic, Complementary, MutuallyExclusive, Implication,
    BooleanAnd, BooleanOr, BooleanNot, ConditionalIf, BooleanExpression }
impl RelationshipType {
    pub fn wire(self) -> &'static str { match self {
        Self::Monotonic=>"monotonic", Self::Complementary=>"complementary",
        Self::MutuallyExclusive=>"mutually_exclusive", Self::Implication=>"implication",
        Self::BooleanAnd=>"boolean_and", Self::BooleanOr=>"boolean_or",
        Self::BooleanNot=>"boolean_not", Self::ConditionalIf=>"conditional_if",
        Self::BooleanExpression=>"boolean_expression",
    }}
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipStatus { Active, Stale, Disabled, Settled, Deleted }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSource { AutoDerived, AdminTagged }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole { Member, Antecedent, Consequent, Output, Input }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionKind { AntecedentToConsequent, ConditionConsequenceToOutput,
    InputsToOutput, Symmetric, OrderedMembers }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Direction { pub kind: DirectionKind, pub from_exchange_ids: Vec<String>,
    pub to_exchange_ids: Vec<String>, pub description: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expression {
    Leaf { #[serde(rename="exchangeId")] exchange_id: u64 },
    And { children: Vec<Expression> }, Or { children: Vec<Expression> },
    Not { child: Box<Expression> },
    Cond { condition: Box<Expression>, consequence: Box<Expression> },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node { pub exchange_id: String, pub market_id: Option<String>,
    pub market_title: Option<String>, pub outcome: Option<String>,
    pub current_price: Option<serde_json::Number>, pub role: NodeRole,
    pub contract_id: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSemantics { pub description: String, pub price_rule: String,
    pub bound_semantics: String, pub evaluation_endpoint: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relationship { pub id: String, pub r#type: RelationshipType,
    pub source: RelationshipSource, pub status: RelationshipStatus,
    pub label: Option<String>, pub reason: Option<String>, pub is_exhaustive: bool,
    pub expression: Option<Expression>, pub nodes: Vec<Node>, pub direction: Direction,
    pub constraint: ConstraintSemantics, pub created_at: Option<String>,
    pub updated_at: Option<String>, pub version: u64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipList { pub data: Vec<Relationship>, pub pagination: Pagination }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination { pub limit: u16, pub has_more: bool, pub next_cursor: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph { pub relationships: Vec<Relationship>, pub nodes: Vec<Node> }

fn typed<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|_| fail("RELATIONSHIP_SCHEMA"))
}
fn required(v:&Value,fields:&[&str])->Result<()> {need(fields.iter().all(|k|v.get(*k).is_some()),"RELATIONSHIP_REQUIRED_FIELD")}
fn required_node(v:&Value)->Result<()> {required(v,&["exchangeId","marketId","marketTitle","outcome","currentPrice","role","contractId"])}
fn required_relationship(v:&Value)->Result<()> {
    required(v,&["id","type","source","status","label","reason","isExhaustive","expression","nodes","direction","constraint","createdAt","updatedAt","version"])?;
    for n in v["nodes"].as_array().ok_or_else(||fail("RELATIONSHIP_NODES"))? {required_node(n)?;}Ok(())
}
fn number(v: &serde_json::Number, nonnegative: bool) -> Result<i128> {
    let n=decimal_units(&v.to_string())?;
    need(!nonnegative || n>=0,"RELATIONSHIP_NEGATIVE_ECONOMICS")?; Ok(n)
}
fn scoped_node(node: &Node, scope: &Scope) -> Result<()> {
    let market=scope.exchanges.get(&node.exchange_id).ok_or_else(||fail("RELATIONSHIP_EXCHANGE_SCOPE"))?;
    need(node.market_id.as_ref().is_none_or(|m|m==market),"RELATIONSHIP_MARKET_SCOPE")?;
    if let Some(price)=&node.current_price { need((0..=1_000_000).contains(&number(price,true)?),"RELATIONSHIP_PRICE")?; }
    Ok(())
}
fn references(direction:&Direction, ids:&BTreeSet<&str>) -> Result<()> {
    for refs in [&direction.from_exchange_ids,&direction.to_exchange_ids] {
        let mut seen=BTreeSet::new();
        for id in refs { need(ids.contains(id.as_str())&&seen.insert(id),"RELATIONSHIP_REFERENCE")?; }
    } Ok(())
}
fn expression_refs(expr:&Expression,ids:&BTreeSet<&str>,depth:usize,budget:&mut usize)->Result<()> {
    need(depth<=32&&*budget<4096,"RELATIONSHIP_AST_BUDGET")?; *budget+=1;
    match expr {
        Expression::Leaf{exchange_id}=>need(ids.contains(exchange_id.to_string().as_str()),"RELATIONSHIP_AST_REFERENCE"),
        Expression::And{children}|Expression::Or{children}=>{need(!children.is_empty(),"RELATIONSHIP_EMPTY_AST")?;for child in children {expression_refs(child,ids,depth+1,budget)?;}Ok(())},
        Expression::Not{child}=>expression_refs(child,ids,depth+1,budget),
        Expression::Cond{condition,consequence}=>{expression_refs(condition,ids,depth+1,budget)?;expression_refs(consequence,ids,depth+1,budget)},
    }
}
fn relationship(row:&Relationship,scope:&Scope)->Result<()> {
    need(uuid(&row.id)&&row.version>0&&!row.nodes.is_empty()&&row.nodes.len()<=1024,"RELATIONSHIP_ID_OR_VERSION")?;
    let mut ids=BTreeSet::new();for node in &row.nodes {scoped_node(node,scope)?;need(ids.insert(node.exchange_id.as_str()),"RELATIONSHIP_DUPLICATE_NODE")?;}
    references(&row.direction,&ids)?;
    for time in [&row.created_at,&row.updated_at].into_iter().flatten() {need(parse_clock(&Value::String(time.clone())).is_some(),"RELATIONSHIP_CLOCK")?;}
    need((row.r#type==RelationshipType::BooleanExpression)==row.expression.is_some(),"RELATIONSHIP_EXPRESSION_TYPE")?;
    if let Some(expr)=&row.expression {expression_refs(expr,&ids,0,&mut 0)?;}
    // This URL is documentation, never a network destination. Even if it points
    // elsewhere it cannot expand the fixed endpoint enum or disclose credentials.
    Ok(())
}
pub fn parse_list(v:&Value,scope:&Scope)->Result<RelationshipList> {
    required(&v["pagination"],&["limit","hasMore","nextCursor"])?;
    for row in v["data"].as_array().ok_or_else(||fail("RELATIONSHIP_ROWS"))? {required_relationship(row)?;}
    let result:RelationshipList=typed(v)?;
    need((1..=200).contains(&result.pagination.limit)&&result.data.len()<=200_000,"RELATIONSHIP_PAGE_LIMIT")?;
    let pg=&result.pagination;
    need(if pg.has_more {pg.next_cursor.as_ref().is_some_and(|c|!c.is_empty()&&c.len()<=4096&&!c.chars().any(char::is_control))&&!result.data.is_empty()}else{pg.next_cursor.is_none()},"RELATIONSHIP_CURSOR")?;
    let mut ids=BTreeSet::new();for row in &result.data {relationship(row,scope)?;need(ids.insert(&row.id),"RELATIONSHIP_DUPLICATE_ID")?;} Ok(result)
}
pub fn parse_graph(v:&Value,scope:&Scope)->Result<Graph> {
    for row in v["relationships"].as_array().ok_or_else(||fail("RELATIONSHIP_ROWS"))? {required_relationship(row)?;}
    for row in v["nodes"].as_array().ok_or_else(||fail("RELATIONSHIP_NODES"))? {required_node(row)?;}
    let graph:Graph=typed(v)?; need(graph.nodes.len()<=1024&&graph.relationships.len()<=4096,"RELATIONSHIP_GRAPH_BUDGET")?;
    let mut nodes=BTreeSet::new();for node in &graph.nodes {scoped_node(node,scope)?;need(nodes.insert(node.exchange_id.as_str()),"RELATIONSHIP_DUPLICATE_NODE")?;}
    let mut ids=BTreeSet::new();for row in &graph.relationships {relationship(row,scope)?;need(ids.insert(&row.id),"RELATIONSHIP_DUPLICATE_ID")?;for n in &row.nodes{need(nodes.contains(n.exchange_id.as_str()),"RELATIONSHIP_GRAPH_NODE_MISSING")?;}}
    Ok(graph)
}

impl Relationship {
    /// Structural preconditions only. A solver must additionally verify exact
    /// settlement/refund semantics, allowed joint states and executable depth.
    pub fn trading_semantics_ready(&self)->bool {
        self.status==RelationshipStatus::Active && self.nodes.iter().all(|n|n.market_id.is_some()&&n.contract_id.is_some()&&n.outcome.as_deref().is_some_and(|s|!s.is_empty()))
    }
}

#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct CollateralComponent {
    pub profile_id:String,pub tournament_id:String,pub component_id:String,
    pub covered_exchange_ids:Vec<String>,pub relationship_ids:Vec<String>,
    pub outstanding_advance_units:i128,pub guaranteed_payout_floor_units:Option<i128>,
    pub diagnostic:Option<String>,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Collateral {pub tournament_id:String,pub available_advance_units:i128,
    pub outstanding_liability_units:i128,pub components:Vec<CollateralComponent>}
pub fn parse_collateral(v:&Value,scope:&Scope)->Result<Collateral> {
    let available=crate::money_value(&v["availableAdvance"])?;need(available>=0,"COLLATERAL_ADVANCE")?;
    let rows=v["data"].as_array().ok_or_else(||fail("COLLATERAL_ROWS"))?;
    need(rows.len()<=4096,"COLLATERAL_COMPONENT_LIMIT")?;let mut components=vec![];let mut ids=BTreeSet::new();let mut total=0i128;
    for row in rows {
        need(row["tournamentId"]==scope.tournament_id,"COLLATERAL_TOURNAMENT_SCOPE")?;
        let profile=row["profileId"].as_str().filter(|s|uuid(s)).ok_or_else(||fail("COLLATERAL_PROFILE"))?;
        let component=row["componentId"].as_str().filter(|s|!s.is_empty()&&s.len()<=4096&&!s.chars().any(char::is_control)).ok_or_else(||fail("COLLATERAL_COMPONENT"))?;
        need(ids.insert(component),"COLLATERAL_DUPLICATE_COMPONENT")?;
        let liability=crate::money_value(&row["outstandingAdvance"])?;need(liability>=0,"COLLATERAL_LIABILITY")?;
        total=total.checked_add(liability).ok_or_else(||fail("COLLATERAL_OVERFLOW"))?;
        let floor=match row.get("guaranteedPayoutFloor"){Some(Value::Null)=>None,Some(v)=>{let n=crate::money_value(v)?;need(n>=0,"COLLATERAL_FLOOR")?;Some(n)},None=>return Err(fail("COLLATERAL_FLOOR_MISSING"))};
        let diagnostic=match row.get("diagnostic"){Some(Value::Null)=>None,Some(Value::String(s))=>Some(s.clone()),_=>return Err(fail("COLLATERAL_DIAGNOSTIC"))};
        let mut exchanges=vec![];let mut seen=BTreeSet::new();for e in row["coveredExchangeIds"].as_array().ok_or_else(||fail("COLLATERAL_EXCHANGES"))? {
            let id=e.as_u64().filter(|i|*i>0&&*i<=9_007_199_254_740_991).ok_or_else(||fail("COLLATERAL_EXCHANGE"))?.to_string();
            need(scope.exchanges.contains_key(&id)&&seen.insert(id.clone()),"COLLATERAL_EXCHANGE_SCOPE")?;exchanges.push(id);
        }
        let mut relationships=vec![];let mut seen=BTreeSet::new();for id in row["relationshipIds"].as_array().ok_or_else(||fail("COLLATERAL_RELATIONSHIPS"))? {
            let id=id.as_str().filter(|s|uuid(s)).ok_or_else(||fail("COLLATERAL_RELATIONSHIP_ID"))?;
            need(seen.insert(id),"COLLATERAL_DUPLICATE_RELATIONSHIP")?;relationships.push(id.to_string());
        }
        components.push(CollateralComponent{profile_id:profile.into(),tournament_id:scope.tournament_id.clone(),component_id:component.into(),covered_exchange_ids:exchanges,relationship_ids:relationships,outstanding_advance_units:liability,guaranteed_payout_floor_units:floor,diagnostic});
    }
    Ok(Collateral{tournament_id:scope.tournament_id.clone(),available_advance_units:available,outstanding_liability_units:total,components})
}

#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct BuyingPower {pub tournament_id:String,pub gross_cash_units:Option<i128>,pub available_advance_units:i128,pub buying_power_units:Option<i128>}
pub fn parse_buying_power(v:&Value,scope:&Scope)->Result<BuyingPower> {
    fn nullable(v:&Value,key:&str)->Result<Option<i128>>{match v.get(key){Some(Value::Null)=>Ok(None),Some(n)=>Ok(Some(crate::money_value(n)?)),None=>Err(fail("ACCOUNT_ECONOMICS_MISSING"))}}
    let cash=nullable(v,"balance")?;let power=nullable(v,"buyingPower")?;let advance=crate::money_value(&v["availableAdvance"])?;
    need(advance>=0&&cash.is_some()==power.is_some(),"ACCOUNT_ECONOMICS")?;
    if let (Some(cash),Some(power))=(cash,power){need(cash.max(0).checked_add(advance)==Some(power),"ACCOUNT_BUYING_POWER_MISMATCH")?;}
    Ok(BuyingPower{tournament_id:scope.tournament_id.clone(),gross_cash_units:cash,available_advance_units:advance,buying_power_units:power})
}

/// Per-execution ALL accounting, separate from gross cash and realized P&L.
/// The own-component fields are not a complete account-wide collateral census:
/// collateralSavings can include borrowing from other eligible components.
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ExecutionEconomics {
    pub full_notional_cost_units:i128,pub effective_entry_cost_units:i128,
    pub collateral_savings_units:i128,pub relationship_ids:Vec<String>,
    pub component_id:Option<String>,pub guaranteed_payout_floor_after_units:i128,
    pub outstanding_advance_after_units:i128,pub collateral_repayment_units:i128,
    pub redemption_credit_units:i128,pub net_buying_power_impact_units:i128,
}
pub fn parse_execution_economics(v:&Value)->Result<Option<ExecutionEconomics>> {
    if v.is_null(){return Ok(None);}
    required(v,&["fullNotionalCost","effectiveEntryCost","collateralSavings","relationshipIds","componentId","guaranteedPayoutFloorAfter","outstandingAdvanceAfter","collateralRepayment","redemptionCredit","netBuyingPowerImpact"])?;
    let positive=|key:&str|->Result<i128>{let n=crate::money_value(&v[key])?;need(n>=0,"EXECUTION_ECONOMICS_NEGATIVE")?;Ok(n)};
    let full=positive("fullNotionalCost")?;let effective=positive("effectiveEntryCost")?;let savings=positive("collateralSavings")?;
    need(full.checked_sub(effective)==Some(savings),"EXECUTION_ECONOMICS_NOTIONAL")?;
    let component=match &v["componentId"]{Value::Null=>None,Value::String(s) if !s.is_empty()&&s.len()<=4096&&!s.chars().any(char::is_control)=>Some(s.clone()),_=>return Err(fail("EXECUTION_ECONOMICS_COMPONENT"))};
    let mut relationships=vec![];let mut seen=BTreeSet::new();for id in v["relationshipIds"].as_array().ok_or_else(||fail("EXECUTION_ECONOMICS_RELATIONSHIPS"))? {
        let id=id.as_str().filter(|s|uuid(s)).ok_or_else(||fail("EXECUTION_ECONOMICS_RELATIONSHIP"))?;need(seen.insert(id),"EXECUTION_ECONOMICS_DUPLICATE_RELATIONSHIP")?;relationships.push(id.into());
    }
    Ok(Some(ExecutionEconomics{full_notional_cost_units:full,effective_entry_cost_units:effective,collateral_savings_units:savings,relationship_ids:relationships,component_id:component,guaranteed_payout_floor_after_units:positive("guaranteedPayoutFloorAfter")?,outstanding_advance_after_units:positive("outstandingAdvanceAfter")?,collateral_repayment_units:positive("collateralRepayment")?,redemption_credit_units:positive("redemptionCredit")?,net_buying_power_impact_units:crate::money_value(&v["netBuyingPowerImpact"])?}))
}

/// Constraint observations deliberately retain exact official nullable economics
/// and recommendations as data, never dispatch them or dereference supplied URLs.
pub fn validate_constraints(v:&Value,scope:&Scope)->Result<()> {
    need(parse_clock(&v["computedAt"]).is_some()&&v["violationsCount"].as_u64().is_some(),"RELATIONSHIP_CONSTRAINT_CLOCK")?;
    let rows=v["data"].as_array().ok_or_else(||fail("RELATIONSHIP_CONSTRAINT_ROWS"))?;need(rows.len()<=4096,"RELATIONSHIP_CONSTRAINT_LIMIT")?;
    let mut ids=BTreeSet::new();
    for row in rows {
        required(row,&["relationshipId","type","violationAmount","reason","suggestedCorrectiveTrades","observedPrices","bound","lowerBound","upperBound","currentValue","evaluationStatus","diagnostic","tournamentId","direction","constraint"])?;
        let id=row["relationshipId"].as_str().filter(|s|uuid(s)).ok_or_else(||fail("RELATIONSHIP_CONSTRAINT_ID"))?;need(ids.insert(id)&&row["tournamentId"]==scope.tournament_id,"RELATIONSHIP_CONSTRAINT_SCOPE")?;
        let _:RelationshipType=typed(&row["type"])?;
        need(row["reason"].is_string()&&(row["diagnostic"].is_null()||row["diagnostic"].is_string()),"RELATIONSHIP_CONSTRAINT_REASON")?;
        // Constraint calculations can contain sub-micro-unit residuals. Keep
        // original Number lexemes; f64 is used only for finite/range checks,
        // never for order costs, a payout certificate or the financial ledger.
        let amount=row["violationAmount"].as_f64().filter(|x|x.is_finite()&&*x>=0.0).ok_or_else(||fail("RELATIONSHIP_VIOLATION_AMOUNT"))?;
        match row["evaluationStatus"].as_str(){Some("violated")=>need(amount>0.0,"RELATIONSHIP_VIOLATION_AMOUNT")?,Some("satisfied"|"unevaluable")=>need(amount==0.0,"RELATIONSHIP_VIOLATION_AMOUNT")?,_=>return Err(fail("RELATIONSHIP_EVALUATION_STATUS"))};
        for key in ["bound","lowerBound","upperBound","currentValue"] {match row.get(key){Some(Value::Null)=>(),Some(n)=>{let value=n.as_f64().filter(|x|x.is_finite()).ok_or_else(||fail("RELATIONSHIP_CONSTRAINT_BOUND"))?;if key=="lowerBound"||key=="upperBound"{need((0.0..=1.0).contains(&value),"RELATIONSHIP_CONSTRAINT_BOUND")?;}},None=>return Err(fail("RELATIONSHIP_CONSTRAINT_MISSING"))}}
        let direction:Direction=typed(&row["direction"])?;let ids=scope.exchanges.keys().map(String::as_str).collect();references(&direction,&ids)?;
        let _:ConstraintSemantics=typed(&row["constraint"])?;
        for key in ["observedPrices","suggestedCorrectiveTrades"] {
            let values=match row.get(key){Some(Value::Null) if key=="observedPrices"=>continue,Some(Value::Array(a))=>a,_=>return Err(fail("RELATIONSHIP_CONSTRAINT_REFERENCES"))};
            for value in values {let e=value["exchangeId"].as_str().filter(|s|numeric(s)).ok_or_else(||fail("RELATIONSHIP_CONSTRAINT_EXCHANGE"))?;need(scope.exchanges.get(e).is_some_and(|m|value["marketId"].is_null()||value["marketId"]==*m),"RELATIONSHIP_CONSTRAINT_EXCHANGE_SCOPE")?;if key=="observedPrices"{need((0..=1_000_000).contains(&crate::money_value(&value["price"])?),"RELATIONSHIP_PRICE")?;} }
        }
    }
    Ok(())
}

#[cfg(test)]mod tests;
