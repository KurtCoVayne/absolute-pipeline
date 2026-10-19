# The house presets (docs/design/agent-guidelines.md, section 15; the
# amendment of PL-8.1 puts them in corpus-v2/lib/execution.dsl). The gateway
# adds this source to a program's workspace when its library defines
# neither, so that a program naming them checks against the pinned tree.
# Where the guideline's text does not parse on the pinned tree (`commission
# per_contract 3.75`, `numerics exact`, `limits { ... }`, a daily `moo_moc`),
# the nearest the grammar takes stands until the amendment lands.

execution futures_house {
  price         close
  orders        moo_moc
  lot           fractional
  capital       1_000_000 USD fixed
  commission    per_contract from catalog
  slippage      none
  impact        none
  participation none
  financing     none
  leverage      unconstrained
  actions       apply
  delisting     last_price
  oversize      halt
  ruin          halt
  numerics      two_pass
  settlement    none
}

execution equities_house {
  price         close
  orders        moc                       # the guideline writes moo_moc; the kernel takes it on a sub-daily base only (WF-14)
  lot           fractional
  capital       1_000_000 USD fixed
  commission    per_share 0.005 USD min 1 USD
  slippage      none
  impact        none
  participation none
  financing     none
  leverage      unconstrained
  actions       apply
  delisting     last_price
  oversize      halt
  ruin          halt
  numerics      two_pass
}
