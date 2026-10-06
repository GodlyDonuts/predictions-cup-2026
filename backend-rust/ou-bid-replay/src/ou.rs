// Irregular-time Euler OU fit. The stress is a model scenario, not a calibrated bound.
#[derive(Debug)]pub struct Forecast{pub stressed_bid:f64,pub mean:f64,pub half_life_seconds:f64}
pub fn forecast(samples:&[(i64,f64)],current_bid:f64,request:i64)->Option<Forecast>{
 if samples.len()<20||samples.last()?.0-samples.first()?.0<1_800_000||samples.iter().any(|(t,p)|*t>=request||!p.is_finite()||*p<=0.||*p>=1.){return None}
 let mut changes=0;let mut total=0.;let mut sx=0.;let mut dy=0.;let mut pairs=Vec::new();
 for pair in samples.windows(2){let dt=(pair[1].0-pair[0].0)as f64/1000.;if !(0.001..=300.).contains(&dt){return None}let x=pair[0].1;let delta=pair[1].1-x;changes+=usize::from(delta.abs()>1e-12);total+=dt;sx+=dt*x;dy+=delta;pairs.push((dt,x,delta/dt));}
 if changes<5||!current_bid.is_finite()||current_bid<=0.||current_bid>=1.{return None}
 let mx=sx/total;let my=dy/total;let xx=pairs.iter().map(|(dt,x,_)|dt*(x-mx).powi(2)).sum::<f64>();if xx<=1e-12{return None}
 let slope=pairs.iter().map(|(dt,x,y)|dt*(x-mx)*(y-my)).sum::<f64>()/xx;let kappa=-slope;if kappa<=0.||!kappa.is_finite(){return None}let alpha=my-slope*mx;let center=alpha/kappa;
 let noise=pairs.iter().map(|(dt,x,y)|dt*(y-alpha-slope*x).powi(2)).sum::<f64>()/(pairs.len()-2)as f64;let se=(noise/xx).sqrt();let half_life=std::f64::consts::LN_2/kappa;
 if kappa<=se||pairs.iter().any(|(dt,_,_)|kappa*dt>0.5)||!(60.0..=7200.0).contains(&half_life)||!(0.005..=0.995).contains(&center){return None}
 let decay=(-kappa*7200.).exp();let mean=center+(current_bid-center)*decay;let variance=(noise/(2.*kappa)*(1.-decay*decay)).max(0.0025_f64.powi(2));let stressed=mean-variance.sqrt();
 if !stressed.is_finite()||!(0.005..=0.995).contains(&stressed){return None}Some(Forecast{stressed_bid:stressed,mean,half_life_seconds:half_life})
}
#[cfg(test)]mod tests{
 use super::*;
 fn history()->Vec<(i64,f64)>{(0..=36).map(|i|(i*60_000,0.5+0.2*0.98_f64.powi(i as i32))).collect()}
 #[test]fn known_ou_parameters_and_stress_are_recovered_without_future_labels(){let h=history();let f=forecast(&h,0.45,2_200_000).unwrap();assert!((f.half_life_seconds-std::f64::consts::LN_2*3000.).abs()<1e-6);assert!((f.mean-(0.5-0.05*(-2.4_f64).exp())).abs()<1e-10);assert!((f.mean-f.stressed_bid-0.0025).abs()<1e-10);}
 #[test]fn flat_trending_future_and_gapped_histories_are_rejected(){let flat=(0..=36).map(|i|(i*60_000,0.5)).collect::<Vec<_>>();assert!(forecast(&flat,0.45,2_200_000).is_none());let trend=(0..=36).map(|i|(i*60_000,0.4+i as f64*0.005)).collect::<Vec<_>>();assert!(forecast(&trend,0.45,2_200_000).is_none());assert!(forecast(&history(),0.45,1_000_000).is_none());let mut gap=history();gap[20].0+=600_000;assert!(forecast(&gap,0.45,3_000_000).is_none());}
}
