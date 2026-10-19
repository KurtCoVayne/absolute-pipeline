# quant-idea-backtest

Skill + engine to test published trading ideas (X links, articles, screenshots) on Jacob's 49-futures
universe with CSI daily back-adjusted data.

Quick start (Claude Code, from the skill folder, data in D:\claude):

    python scripts/fetch_tweet.py https://x.com/<user>/status/<id> --out work/idea/tweet
    python scripts/run_backtest.py --strategy strategies.zscore_scalein_mr:ZScoreScaleInMR \
        --data D:/claude --out work/idea/base --title "Z-score scale-in MR" \
        --sensitivity '{"lookback":[3,5,8,10,15],"z_entry":[-0.5,-1,-1.5,-2]}'

Requirements: python 3.10+, pandas, numpy, scipy, matplotlib.

Layout: SKILL.md (workflow) · references/conventions.md (data, sizing, costs, metrics) ·
references/protocol.md (quant checklist) · assets/universe.csv · scripts/qbt (engine) ·
scripts/strategies (one file per idea; template.py to start a new one).
