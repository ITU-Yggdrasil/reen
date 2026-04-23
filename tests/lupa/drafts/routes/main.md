# Routes: main

## Purpose

Defines the top-level navigation structure of the Lupa banking application. All routes except the login screen require an authenticated session. The transfer hub uses nested paths to deep-link into specific transfer types while keeping tab state synchronised.

## Routes

- path: /login
  component: LoginScreen
  auth: none

- path: /
  component: AccountsScreen
  auth: required

- path: /accounts/{id}
  component: AccountDetailScreen
  auth: required
  params:
    id: Account

- path: /transfer
  component: TransferHubScreen
  auth: required

- path: /transfer/own
  component: TransferHubScreen
  auth: required

- path: /transfer/domestic
  component: TransferHubScreen
  auth: required

- path: /transfer/international
  component: TransferHubScreen
  auth: required

- path: /chat
  component: ChatScreen
  auth: required
