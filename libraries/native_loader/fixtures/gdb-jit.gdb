set disable-randomization off
set breakpoint pending on
break xila_jit_probe
run
python assert gdb.newest_frame().name() == "xila_jit_probe"
disable 1
continue
python assert int(gdb.parse_and_eval("$_exitcode")) == 0
