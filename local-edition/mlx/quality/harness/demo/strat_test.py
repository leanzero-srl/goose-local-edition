p='/Users/mihaiperdum/Projects/goose/.claude/worktrees/agent-a7b0919188cbd8067/ui/desktop/src/components/nodes/StrategiesTab.test.tsx'
s=open(p).read()
a=s.index("  it('Remove: a refusal offers its way through and the second try carries it'")
b=s.index("describe('the strategy editor'")
new='''  it('Remove (Q-259): what the config already says is a box on open — no red, Remove disabled with its reason', async () => {
    mockRemove.mockResolvedValueOnce({ written: true, refusals: [], read: readOf(CONFIG) });
    renderTab();
    await userEvent.click(within(card('everyday')).getByTestId('strategy-more'));
    await userEvent.click(await screen.findByTestId('strategy-remove'));
    const dialog = await screen.findByTestId('strategy-remove-dialog');
    expect(dialog).toHaveTextContent('Remove Everyday?');
    // Nothing was tried: no error, no "Not removed".
    expect(screen.queryByTestId('strategy-remove-refusals')).toBeNull();
    expect(screen.queryByText('Not removed')).toBeNull();
    // New chats start on Everyday (CONFIG.forNewChats): the box says what ticking it does, and why.
    const box = screen.getByTestId('strategy-remove-and-auto');
    expect(box).toHaveTextContent('Start new chats on Any node (Auto) instead');
    expect(box).toHaveTextContent(
      'New chats start on this strategy now, so it can’t be removed on its own.'
    );
    expect(screen.queryByTestId('strategy-remove-and-pool')).toBeNull();
    // Remove is disabled, and says why beside it — never a click that silently does nothing.
    const confirm = screen.getByTestId('strategy-remove-confirm');
    expect(confirm).toBeDisabled();
    expect(screen.getByTestId('strategy-remove-blocked')).toHaveTextContent(
      'Tick the box above to remove it'
    );
    await userEvent.click(confirm);
    expect(mockRemove).not.toHaveBeenCalled();

    await userEvent.click(box);
    expect(screen.queryByTestId('strategy-remove-blocked')).toBeNull();
    expect(confirm).toBeEnabled();
    await userEvent.click(confirm);
    expect(mockRemove).toHaveBeenLastCalledWith('everyday', {
      andNewChatsAuto: true,
      andBuildsPool: false,
    });
    await waitFor(() => expect(screen.queryByTestId('strategy-remove-dialog')).toBeNull());
    expect(mockRefresh).toHaveBeenCalled();
  });

  it('Remove: an engine refusal a box answers becomes that box (never red); any other is red, after the attempt, in its words', async () => {
    mockRemove
      .mockResolvedValueOnce({
        written: false,
        refusals: [
          // Swarm builds moved to Quick after the dialog's read: the engine says so.
          { code: 'strategyIsForBuilds', message: 'swarm builds use "Quick"; remove it and …' },
        ],
        read: readOf(CONFIG),
      })
      .mockResolvedValueOnce({
        written: false,
        refusals: [{ code: 'unknownStrategy', message: "there is no strategy 'quick'" }],
        read: readOf(CONFIG),
      });
    renderTab();
    await userEvent.click(within(card('quick')).getByTestId('strategy-more'));
    await userEvent.click(await screen.findByTestId('strategy-remove'));
    await screen.findByTestId('strategy-remove-dialog');
    // Quick is neither for new chats nor for builds in this config: nothing to confirm.
    expect(screen.queryByTestId('strategy-remove-confirmations')).toBeNull();
    await userEvent.click(screen.getByTestId('strategy-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith('quick', {
      andNewChatsAuto: false,
      andBuildsPool: false,
    });
    const pool = await screen.findByTestId('strategy-remove-and-pool');
    expect(pool).toHaveTextContent('Let swarm builds use your swarm pool instead');
    expect(screen.queryByTestId('strategy-remove-refusals')).toBeNull();
    expect(screen.getByTestId('strategy-remove-dialog').parentElement).not.toHaveTextContent(
      'swarm builds use "Quick"'
    );
    expect(screen.getByTestId('strategy-remove-confirm')).toBeDisabled();
    await userEvent.click(pool);
    await userEvent.click(screen.getByTestId('strategy-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith('quick', {
      andNewChatsAuto: false,
      andBuildsPool: true,
    });
    const refusals = await screen.findByTestId('strategy-remove-refusals');
    expect(refusals).toHaveTextContent('Not removed');
    expect(refusals).toHaveTextContent("there is no strategy 'quick'");
  });
});

'''
s=s[:a]+new+s[b:]
open(p,'w').write(s)
