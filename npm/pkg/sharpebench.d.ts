/* tslint:disable */
/* eslint-disable */

export function audit_briefing(briefing_json: string, policy_json: string): string;

export function canary(seed: string): string;

export function greeks(params_json: string): string;

export function is_my_sharpe_real(returns_json: string, config_json: string): string;

export function is_my_sharpe_real_full(field_json: string, winner_idx: number, config_json: string): string;

export function score(submissions_json: string, config_json: string): string;

export function score_agent(submission_json: string, config_json: string): string;

export function score_allocation(trajectory_json: string, policy_json: string): string;

export function self_audit(): string;
