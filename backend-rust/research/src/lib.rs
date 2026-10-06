// baal-research modules: research/ offline analysis port.
#![allow(clippy::too_many_arguments)]
#![allow(non_snake_case)]
#![allow(dead_code)]
mod js;
mod files;
pub mod book_alpha_v1;
pub mod current_election__assessment;
pub mod current_election__prepare_bank;
pub mod current_election__prepare_tennessee;
pub mod current_polls_v1__assessment;
pub mod current_polls_v1__build;
pub mod current_polls_v1__capture;
pub mod current_polls_v1__compare;
pub mod current_polls_v2__assessment;
pub mod current_polls_v2__build;
pub mod current_polls_v2__capture;
pub mod current_polls_v2__capture_comparators;
pub mod current_polls_v2__compare;
pub mod current_polls_v2__party_assessment;
pub mod current_polls_v3__capture;
pub mod current_polls_v3__prepare_texas_governor;
pub mod election_baseline_v1__download_sources;
pub mod election_baseline_v1__model;
pub mod election_baseline_v1__train;
pub mod election_baseline_v2__download_sources;
pub mod election_baseline_v2__model;
pub mod election_baseline_v2__screen;
pub mod election_baseline_v2__train;
pub mod governor_error_v1__build;
pub mod governor_error_v1__model;
pub mod median_barrier_diagnostic;
pub mod paired_forecast_v1;
pub mod poll_calibration_v1__capture;
pub mod poll_calibration_v1__frozen_model_baseline_20261005;
pub mod poll_calibration_v1__model;
pub mod poll_calibration_v1__prepare_kansas_paper;
pub mod poll_calibration_v1__train;
pub mod poll_horizon_v2;
pub mod profit_sprint_20261005__capture;
pub mod profit_sprint_20261005__compare;
pub mod profit_sprint_20261005__identities;
pub mod profit_sprint_20261005__verify;
