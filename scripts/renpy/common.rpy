# scripts/renpy-check が書き出し先の game/ に置く、自動プレイの共通部品（コミットするのは scn-studio の側だけ）。
# 通ったラベルを覚え、testcase の `eval` で調べる。

init python:
    scn_seen = set()

    def _scn_label(name, abnormal):
        scn_seen.add(name)

    config.label_callbacks.append(_scn_label)

testsuite global:
    setup:
        $ _test.transition_timeout = 0.05
    teardown:
        exit
