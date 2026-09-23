import assert from 'node:assert/strict';

export async function runBoardsTests({ check, open }) {
  await check('Boards dashboard creates boards and custom columns', async page => {
    await open(page, '/boards');
    await page.getByRole('heading', { name: 'Boards', exact: true }).waitFor();
    await page.locator('summary').filter({ hasText: 'New board' }).click();
    await page.getByLabel('New board name', { exact: true }).fill('Project follow-ups');
    await page.getByRole('button', { name: 'Create', exact: true }).click();
    await page.locator('.board-list-item').filter({ hasText: 'Project follow-ups' }).waitFor();

    await page.locator('summary').filter({ hasText: 'Add column' }).click();
    await page.getByLabel('New column name', { exact: true }).fill('Review');
    await page.getByRole('button', { name: 'Add', exact: true }).click();
    await page.locator('.kanban-column').filter({ hasText: 'Review' }).waitFor();

    const boards = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_boards'));
    const project = boards.boards.find(board => board.name === 'Project follow-ups');
    assert.ok(project, 'created board should be persisted in the fixture');
    assert.deepEqual(project.columns.map(column => column.name), ['To Do', 'Review']);
  });

  await check('Meeting action items drill into boards and columns', async page => {
    await open(page, '/meeting/1');
    await page.getByRole('tab', { name: 'Enhance Notes', exact: true }).click();
    await page.evaluate(() => { window.audit.boardAddDelay = 120; });
    await page.getByRole('button', { name: 'Add action item to a board', exact: true }).click();

    const menu = page.getByRole('dialog', { name: 'Add action item to a board', exact: true });
    await menu.getByRole('button', { name: 'Follow up with the team.', exact: true }).click();
    await menu.getByRole('button', { name: /To-Do's/ }).click();
    await menu.getByRole('button', { name: /Backlog/ }).click();
    await page.keyboard.press('Escape');
    assert.equal(await menu.isVisible(), true, 'the menu should stay stable while the add is pending');
    await page.waitForFunction(() => window.audit.calls.some(call => call.cmd === 'add_board_card'));
    await menu.waitFor({ state: 'detached' });
    await page.getByRole('status').filter({ hasText: 'Added Follow up with the team.' }).waitFor();
    assert.equal(
      await page.evaluate(() => document.activeElement?.getAttribute('aria-label')),
      'Add action item to a board',
      'focus should return to the add-to-board trigger',
    );

    const addCall = await page.evaluate(() => window.audit.calls.find(call => call.cmd === 'add_board_card'));
    assert.deepEqual(addCall.args, {
      boardId: 1,
      columnId: 1,
      title: 'Follow up with the team.',
      meetingId: 1,
      sourceActionIndex: 0,
      sourceEnhancedMarkdown: '## Summary\n\nFixture summary.\n\n## Action items\n\n- Follow up with the team.',
    });

    await open(page, '/boards');
    await page.getByText('Follow up with the team.', { exact: true }).waitFor();
    const move = page.getByLabel('Move Follow up with the team.', { exact: true });
    await move.selectOption('5');
    const doneColumn = page.locator('.kanban-column[aria-label="Done"]');
    await doneColumn.getByText('Follow up with the team.', { exact: true }).waitFor();
  });

  await check('Editing an enhanced action item updates its linked board cards', async page => {
    await open(page, '/meeting/1');
    await page.getByRole('tab', { name: 'Enhance Notes', exact: true }).click();
    await page.getByRole('button', { name: 'Add action item to a board', exact: true }).click();
    const menu = page.getByRole('dialog', { name: 'Add action item to a board', exact: true });
    await menu.getByRole('button', { name: 'Follow up with the team.', exact: true }).click();
    await menu.getByRole('button', { name: /To-Do's/ }).click();
    await menu.getByRole('button', { name: /Backlog/ }).click();
    await page.waitForFunction(() => window.audit.calls.some(call => call.cmd === 'add_board_card'));

    await page.getByRole('button', { name: 'Edit action items', exact: true }).click();
    await page.getByLabel('Action item 1', { exact: true }).fill('Send the revised follow-up.');
    await page.getByRole('button', { name: 'Save action items', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Action items updated' }).waitFor();

    const updateCall = await page.evaluate(() => window.audit.calls.find(call => call.cmd === 'update_enhanced_action_items'));
    assert.deepEqual(updateCall.args, {
      meetingId: 1,
      originalMarkdown: '## Summary\n\nFixture summary.\n\n## Action items\n\n- Follow up with the team.',
      enhancedMarkdown: '## Summary\n\nFixture summary.\n\n## Action items\n\n- Send the revised follow-up.',
      edits: [{ sourceIndex: 0, originalTitle: 'Follow up with the team.', title: 'Send the revised follow-up.' }],
    });

    await open(page, '/boards');
    await page.getByText('Send the revised follow-up.', { exact: true }).waitFor();
    assert.equal(await page.getByText('Follow up with the team.', { exact: true }).count(), 0);
  });

  await check('A rejected card move restores the persisted column selection', async page => {
    await open(page, '/boards');
    await page.evaluate(async () => {
      await window.__TAURI_INTERNALS__.invoke('add_board_card', {
        boardId: 1,
        columnId: 1,
        title: 'Retry this move',
        meetingId: 1,
      });
    });
    await page.reload();
    await page.evaluate(() => window.audit.failBoardMove = true);

    const move = page.getByLabel('Move Retry this move', { exact: true });
    await move.selectOption('5');
    await page.getByRole('alert').filter({ hasText: 'Fixture: board card move failed' }).waitFor();
    assert.equal(await move.inputValue(), '1');
  });

  await check('Board cards drag between columns', async page => {
    await open(page, '/boards');
    const cardId = await page.evaluate(async () => {
      const card = await window.__TAURI_INTERNALS__.invoke('add_board_card', {
        boardId: 1,
        columnId: 1,
        title: 'Drag this card',
        meetingId: 1,
      });
      return card.id;
    });
    await page.reload();

    const card = page.locator('.board-card').filter({ hasText: 'Drag this card' });
    const todoCards = page.locator('.kanban-column[aria-label="To Do"] .column-cards');
    await card.dragTo(todoCards);
    await todoCards.getByText('Drag this card', { exact: true }).waitFor();

    const moveCall = await page.evaluate(id => window.audit.calls.findLast(call =>
      call.cmd === 'move_board_card' && call.args.cardId === id,
    ), cardId);
    assert.equal(moveCall.args.columnId, 2);
  });

  await check('Boards keeps source links neutral', async page => {
    await open(page, '/boards');
    const resolveColor = async token => page.evaluate(tokenName => {
      const probe = document.createElement('span');
      probe.style.color = `var(${tokenName})`;
      document.body.append(probe);
      const value = getComputedStyle(probe).color;
      probe.remove();
      return value;
    }, token);
    const muted = await resolveColor('--text-muted');


    await page.evaluate(async () => {
      await window.__TAURI_INTERNALS__.invoke('add_board_card', {
        boardId: 1,
        columnId: 1,
        title: 'Check the source link',
        meetingId: 1,
      });
    });
    await page.reload();
    await page.getByText('Check the source link', { exact: true }).waitFor();
    const source = await page.locator('.card-source').evaluate(element => getComputedStyle(element).color);

    assert.equal(source, muted);
  });

  await check('Boards uses the workspace width and contains narrow-screen scrolling', async page => {
    await open(page, '/boards');
    await page.locator('.kanban-column').first().waitFor();
    assert.equal(await page.getByLabel('New board name', { exact: true }).isVisible(), false);
    assert.equal(await page.getByLabel('New column name', { exact: true }).isVisible(), false);
    await page.evaluate(async () => {
      for (let index = 1; index <= 30; index++) {
        await window.__TAURI_INTERNALS__.invoke('add_board_card', {
          boardId: 1,
          columnId: 1,
          title: `Overflow card ${index}`,
          meetingId: 1,
        });
      }
    });
    await page.reload();
    await page.locator('.kanban-column').first().waitFor();
    for (const { width, height } of [
      { width: 1756, height: 1189 },
      { width: 1200, height: 900 },
      { width: 800, height: 900 },
      { width: 560, height: 900 },
      { width: 420, height: 520 },
      { width: 390, height: 900 },
    ]) {
      await page.setViewportSize({ width, height });
      const geometry = await page.evaluate(() => {
        const main = document.querySelector('.main').getBoundingClientRect();
        const grid = document.querySelector('.kanban-grid').getBoundingClientRect();
        const column = document.querySelector('.kanban-column').getBoundingClientRect();
        const cards = document.querySelector('.kanban-column .column-cards');
        const gridStyle = getComputedStyle(document.querySelector('.kanban-grid'));
        return { mainWidth: main.width, gridWidth: grid.width, left: grid.left - main.left, top: grid.top,
          columnWidth: column.width, columnHeight: column.height, gridHeight: grid.height,
          bottomGap: innerHeight - grid.bottom, scrollbarColor: gridStyle.scrollbarColor,
          cardsClientHeight: cards.clientHeight, cardsScrollHeight: cards.scrollHeight,
          horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
          verticalOverflow: document.documentElement.scrollHeight > innerHeight };
      });
      assert.ok(geometry.left <= 28, JSON.stringify(geometry));
      assert.ok(geometry.gridWidth >= geometry.mainWidth - 56, JSON.stringify(geometry));
      assert.ok(geometry.top < 350, JSON.stringify(geometry));
      assert.ok(geometry.columnWidth >= 220, JSON.stringify(geometry));
      assert.ok(geometry.gridHeight - geometry.columnHeight <= 16, JSON.stringify(geometry));
      assert.ok(geometry.bottomGap >= 0 && geometry.bottomGap <= 30, JSON.stringify(geometry));
      assert.notEqual(geometry.scrollbarColor, 'auto', JSON.stringify(geometry));
      assert.ok(geometry.cardsScrollHeight > geometry.cardsClientHeight, JSON.stringify(geometry));
      assert.equal(geometry.horizontalOverflow, false, `No horizontal document overflow at ${width}`);
      assert.equal(geometry.verticalOverflow, false, `No vertical document overflow at ${width}×${height}`);
    }
    await page.locator('summary').filter({ hasText: 'New board' }).focus();
    await page.keyboard.press('Enter');
    await page.getByLabel('New board name', { exact: true }).fill('Discard this');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    assert.equal(await page.getByLabel('New board name', { exact: true }).isVisible(), false);
    assert.equal(await page.evaluate(() => window.audit.calls.some(call => call.cmd === 'create_board')), false);
    assert.equal(await page.locator('summary').filter({ hasText: 'New board' }).evaluate(element => element === document.activeElement), true);
    await page.locator('summary').filter({ hasText: 'Add column' }).click();
    await page.getByLabel('New column name', { exact: true }).fill('Discard column');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    assert.equal(await page.locator('summary').filter({ hasText: 'Add column' }).evaluate(element => element === document.activeElement), true);
    if (process.env.BOARDS_SCREENSHOT) {
      await page.setViewportSize({ width: 1756, height: 1189 });
      await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
      await page.screenshot({ path: process.env.BOARDS_SCREENSHOT });
    }
  });
}
