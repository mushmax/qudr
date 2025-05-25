import { Action } from '@/app/actions/actions';
import { dataTableSpec } from '@/app/actions/dataTableSpec';
import { contextMenuAtom } from '@/app/atoms/contextMenuAtom';
import { ContextMenuBase, ContextMenuItemAction } from '@/app/gridGL/HTMLGrid/contextMenus/Base';
import { ContextMenuDataTableNested } from '@/app/gridGL/HTMLGrid/contextMenus/GridContextMenuDataTable';
import { DropdownMenuSeparator } from '@/shared/shadcn/ui/dropdown-menu';
import { useRecoilValue } from 'recoil';

export function GridContextMenuDataTableCell() {
  const contextMenu = useRecoilValue(contextMenuAtom);
  const isShowingColumnNames = Boolean(contextMenu.table?.show_columns);

  return (
    <ContextMenuBase>
      <ContextMenuItemAction action={Action.Cut} />
      <ContextMenuItemAction action={Action.Copy} />
      <ContextMenuItemAction action={Action.Paste} />
      <ContextMenuItemAction action={Action.PasteValuesOnly} />
      <ContextMenuItemAction action={Action.PasteFormattingOnly} />
      <ContextMenuItemAction action={Action.CopyAsPng} />
      <ContextMenuItemAction action={Action.DownloadAsCsv} />
      <DropdownMenuSeparator />
      <ContextMenuItemAction action={Action.InsertTableColumnLeft} />
      <ContextMenuItemAction action={Action.InsertTableColumnRight} />
      <ContextMenuItemAction action={Action.RemoveTableColumn} />
      <DropdownMenuSeparator />
      <ContextMenuItemAction action={Action.InsertTableRowAbove} />
      <ContextMenuItemAction action={Action.InsertTableRowBelow} />
      <ContextMenuItemAction action={Action.RemoveTableRow} />
      <DropdownMenuSeparator />
      <ContextMenuItemAction
        action={Action.HideTableColumn}
        labelOverride={dataTableSpec[Action.HideTableColumn].labelVerbose}
      />
      <DropdownMenuSeparator />
      <ContextMenuDataTableNested isShowingColumnNames={isShowingColumnNames} />
    </ContextMenuBase>
  );
}
