cd /home/user/russh-bbs
export SQLX_OFFLINE=true CARGO_INCREMENTAL=0 PATH=$PATH:/root/.cargo/bin
for c in saltmarch emberdeep nullroute long-haul hallpass; do
  for t in sim social_sim sim_crew; do
    if [ -f crates/doors/$c/tests/$t.rs ]; then
      nice -n 15 cargo test --release -p russh-bbs-door-$c --test $t -- --nocapture > /tmp/qa/sim-$c-$t.log 2>&1; echo "$c $t exit $?" >> /tmp/qa/sims.res
    fi
  done
done
echo done >> /tmp/qa/sims.res
