Fixed

- **Power loss no longer leaves the chain naming lost transactions.** The
  connect barrier now device-syncs every archive file a block commit names
  (outputs, inputs and witness, spend slots, both locator families, txids,
  per-tx stats, and parent edges) before it writes the confirmed tip. A
  power cut could leave the tip, a strong bit, or a block's transaction
  range pointing at bytes that never reached the disk. The node then
  refused to start, rejected the next valid block, or wedged on one block
  until the store was wiped. Those syncs go to the configured bulk-IO
  backend (io_uring, the IO pool, or IOCP) together, so they overlap
  instead of queuing.
- **An unpublished archive tail is cut on open.** A body header that fell
  short of its locator refused startup ("body header short of locator"),
  and a header that landed without its rows refused ("overflow missing")
  or kept a transaction that could not be read. For writes made by this
  binary no committed row names that tail, so open keeps the creates past
  the committed ones that still decode and that every body header covers.
  A datadir an older binary tore below a committed block now opens: the
  tip shrinks to the transactions that remain, and the lost create numbers
  are held by tombstones that never count as spent or confirmed, so stale
  spends and address history cannot attach to a later transaction. A short `input.body` header cuts the
  input locator the same way, and an input locator past that prefix is cut.
  A failed archive sync now latches until restart, so a retried fsync
  cannot commit over pages the kernel dropped. A spend checkpoint syncs the
  archive before its page commit can name a spender.
- **An interrupted `tx.head` rebuild rebuilds again.** Segment files with
  no `tx.head/meta` refused startup as a legacy layout and needed a manual
  delete. Open now wipes that directory and rebuilds it from the archive.
- **Small state files are published whole.** Store `meta`, `store.secret`,
  the mempool `meta`, and the unbroadcast list are written to a synced
  temp file and renamed, with the directory synced. `store.secret` is
  created owner-only. A short mempool `meta` left by an older
  binary is moved aside like any other unreadable mempool image.
- **A torn address index rebuilds itself.** When the scripthash
  write-behind reads a torn record after a power cut, it marks the index
  for a full rebuild on the next start instead of failing every few
  blocks. Before, `RBITCOIN_SH_FORCE_REBUILD` was the only remedy.
