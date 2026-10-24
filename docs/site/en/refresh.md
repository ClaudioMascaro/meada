# Refresh and publication

The selected policy is to publish the whole set or publish nothing: any fetch or indexing failure must abort publication and preserve the current generation. Implementation and verification of that guarantee are [planned in #316](https://github.com/ClaudioMascaro/meada/issues/316). Today, operators can chain the job with `sh -ec` to stop on failure; lock and fencing already protect pointer updates. In [planned S4](enterprise.md), eligibility exclusions will be successful decisions before fetch.

<!-- meada-graph:refresh -->

**Refresh lock** (feature `refresh-lock`, which includes `generation-pointer`, Unix only;
`cargo install meada --features refresh-lock`). With the `[refresh_lock]` section, which needs
`[generations]`, the whole refresh runs under a lock in Valkey, and each acquisition gets a
fence, so an overlapping or stalled job never replaces a newer generation with an older one.

```toml
[refresh_lock]
ttl_seconds = 300                                     # default; at least 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

<details markdown="1" class="reference-details">
<summary>Lock acquisition, renewal and loss</summary>

`meada snapshot lock` takes the lock with `SET {<key_prefix>}:lock <random token> NX PX <ttl>`,
adds one to `{<key_prefix>}:fence` and runs the command as its child, with `MEADA_REFRESH_LOCK`
(the token) and `MEADA_REFRESH_FENCE` in its environment. The lock is renewed every third of the
TTL (compare and extend) and released at the end (compare and delete); the exit code is the
child's (128 plus the signal when a signal ended it). A lock already held exits 1 naming the
key, running nothing. When a renewal finds another token, or none succeeded for a whole TTL, the
lock is lost: the child's process group gets SIGTERM (and SIGKILL 10 s later if it is still
alive), and `lock` exits 1 saying it lost it. A SIGTERM, SIGINT, SIGHUP or SIGQUIT to `lock`
reaches the group the same way; once the child exits, whatever is left in its process group gets
SIGKILL (a grandchild that calls `setsid` leaves the group and escapes), and on Linux the child
gets SIGTERM if `lock` dies first. Indexing stays network-free: the child's `meada index`
opens no connection.

</details>

## Publication and fence recovery

<!-- meada-graph:fence-recovery -->

With `[refresh_lock]`, `meada snapshot publish` needs both variables, and before each upload it
checks that the lock still holds the token: if not, it stops, uploading nothing more and writing
no pointer. The pointer script writes only when the lock holds the token and the fence is
greater than the one in `{<key_prefix>}:current`, in one operation. Nodes never swap to a
generation whose fence is lower than the one in their pointer file; without the file, they take
the current one whatever its fence. Meada never lowers or deletes `{<key_prefix>}:fence`. If a
Valkey flush or failover loses it, the next fences are lower than the recorded ones and the
fleet stops: the refused publish exits 1 naming `{<key_prefix>}:fence`, the pull of a node with
a higher fence exits 1 without swapping, and `meada status --generations` on that node prints a
`fence: stuck: ...` line with both fences (its exit code does not change). Run
`meada snapshot fence recover --workspace <manifest>`: under the lock, it raises the counter
to the highest fence in the pointer, retained generations, workspace and valid heartbeats,
using Valkey time to select live nodes. The next publish uses a higher fence and unsticks the
nodes. Valkey `INCR` has a ceiling of 9223372036854775807; recovery refuses floors or recorded
fences above 9223372036854775806 and does not consume the last available acquisition. Reads
and transport continue to accept exact `u64` fences. For an isolated node whose fence
appears in no valid heartbeat, pass it as `--at-least <fence>`, or delete that node's
`<local_dir>/current` so it takes the current generation. Turning `[refresh_lock]` off after nodes recorded fences above 0 stops the fleet too,
since without the lock every fence is 0: delete each node's `<local_dir>/current`.
