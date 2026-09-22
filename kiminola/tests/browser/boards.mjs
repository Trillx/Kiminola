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
    await page.getByRole('button', { name: 'Add action item to a board', exact: true }).click();

    const menu = page.getByRole('dialog', { name: 'Add action item to a board', exact: true });
    await menu.getByRole('button', { name: 'Follow up with the team.', exact: true }).click();
    await menu.getByRole('button', { name: /To-Do's/ }).click();
    await menu.getByRole('button', { name: /Backlog/ }).click();

    const addCall = await page.evaluate(() => window.audit.calls.find(call => call.cmd === 'add_board_card'));
    assert.deepEqual(addCall.args, {
      boardId: 1,
      columnId: 1,
      title: 'Follow up with the team.',
      meetingId: 1,
    });

    await open(page, '/boards');
    await page.getByText('Follow up with the team.', { exact: true }).waitFor();
    const move = page.getByLabel('Move Follow up with the team.', { exact: true });
    await move.selectOption('5');
    const doneColumn = page.locator('.kanban-column[aria-label="Done"]');
    await doneColumn.getByText('Follow up with the team.', { exact: true }).waitFor();
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
    for (const width of [1756, 1200, 800, 560, 390]) {
      await page.setViewportSize({ width, height: 900 });
      const geometry = await page.evaluate(() => {
        const main = document.querySelector('.main').getBoundingClientRect();
        const grid = document.querySelector('.kanban-grid').getBoundingClientRect();
        const column = document.querySelector('.kanban-column').getBoundingClientRect();
        return { mainWidth: main.width, gridWidth: grid.width, left: grid.left - main.left, top: grid.top,
          columnWidth: column.width, overflow: document.documentElement.scrollWidth > innerWidth };
      });
      assert.ok(geometry.left <= 28, JSON.stringify(geometry));
      assert.ok(geometry.gridWidth >= geometry.mainWidth - 56, JSON.stringify(geometry));
      assert.ok(geometry.top < 350, JSON.stringify(geometry));
      assert.ok(geometry.columnWidth >= 220, JSON.stringify(geometry));
      assert.equal(geometry.overflow, false, `No document overflow at ${width}`);
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
