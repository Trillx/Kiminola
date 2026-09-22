import assert from 'node:assert/strict';

export async function runBoardsTests({ check, open }) {
  await check('Boards dashboard creates boards and custom columns', async page => {
    await open(page, '/boards');
    await page.getByRole('heading', { name: 'Boards', exact: true }).waitFor();
    await page.getByText("Your To-Do's board is ready.", { exact: false }).waitFor();

    await page.getByLabel('New board name', { exact: true }).fill('Project follow-ups');
    await page.getByRole('button', { name: 'New board', exact: true }).click();
    await page.locator('.board-list-item').filter({ hasText: 'Project follow-ups' }).waitFor();

    await page.getByLabel('New column name', { exact: true }).fill('Review');
    await page.getByRole('button', { name: 'Add column', exact: true }).click();
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

  await check('Boards reserves gold for the selected board', async page => {
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
    const inkStrong = await resolveColor('--ink-strong');
    const home = await page.locator('.board-home-link').evaluate(element => getComputedStyle(element).color);
    const welcome = await page.locator('.board-welcome strong').evaluate(element => getComputedStyle(element).color);

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

    assert.equal(home, muted);
    assert.equal(source, muted);
    assert.equal(welcome, inkStrong);
  });
}
